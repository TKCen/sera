# SERA 2.0 — Phase 0 Implementation Plan

> **Purpose:** Turn `docs/plan/IMPL-AUDIT.md` §5 (P0 list) into an actionable, code-level breakdown for the Rust workspace. One section per P0 item, with concrete `.rs` file paths, Cargo features, acceptance tests, sequencing, and milestone exit criteria.
> **Status:** Draft — produced 2026-04-12 via orchestrated fan-out. Implementation begins in the next session.
> **Scope:** Phase 0 only. Phase 1+ items are flagged but not expanded.
> **Inputs:** `docs/plan/HANDOFF.md`, `docs/plan/IMPL-AUDIT.md` §1/§4/§5/§6, and the 19 specs under `docs/plan/specs/` (referenced, not re-read cold).

---

## How to use this document

1. **Start at §Sequencing, parallel lanes, and milestones** — it tells you which P0 items unblock which, and which lanes can be fanned out in parallel.
2. **Drill into the P0-N section for the crate you're working on.** Each section is self-contained: rename strategy, files to create, files to modify, Cargo features, workspace deps, acceptance tests, downstream cascade.
3. **Use §Acceptance-test catalog as a traceability matrix.** It maps every IMPL-AUDIT §4.6 design-forward obligation to the exact test function that proves it exists in Phase 0 code. Gaps (items with no matching test) are blockers for M4.
4. **Sub-agents working on a single crate need only read their P0-N section plus the crate source.** Do not cold-read whole specs. Spec context cited in each Wave 1 section references specific sections only.

## Ground rules carried forward from HANDOFF

- `sera-types` is the single first-mover. Everything else cascades from it (§4.1).
- Crate renames land before new types (§4.2): `sera-core → sera-gateway`, `sera-domain → sera-types`, `sera-events → sera-telemetry` (or split).
- `sera-docker` is **absorbed** into `sera-tools`, not renamed in isolation. Do not create a peer `sera-sandbox` crate (§4.3).
- OTel triad (`opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"`) must be pinned with exact-equals. HANDOFF §6.2 is load-bearing.
- `wasmtime = ">=43, <50"` is a loose range; `extism` must not be added (HANDOFF §4.8).
- `sea-orm` is forbidden in `sera-db` (§4, hard boundary 3).
- Beads integrates via `bd` CLI shell-out, not as a Rust dependency (HANDOFF §6.4).

## Document map

- §Sequencing, parallel lanes, and milestones — inter-crate ordering, parallel lane assignments, M0..M4 exit criteria
- §P0-1 · sera-domain → sera-types — first-mover, design-forward primitives
- §P0-2 · sera-events → sera-telemetry — OCSF/OTel rewrite
- §P0-3 · sera-config extensions — figment/schemars, ShadowConfigStore, ConfigVersionLog
- §P0-4 · sera-db / sera-queue split — apalis 0.7 wiring, migration reversibility
- §P0-5 · sera-core → sera-gateway — SQ/EQ envelope, AppServerTransport spine
- §P0-6 · sera-runtime contract migration — TurnOutcome, ContextEngine, four-method lifecycle
- §P0-7 · sera-auth design-forward types + feature gates — argon2, casbin, CapabilityToken
- §P0-8 · sera-docker → sera-tools absorption — SandboxProvider trait, three-layer policy
- §P0-9 · sera-workflow rewrite — WorkflowTask (beads schema), atomic claim, termination triad
- §P0-10 (partial) · infrastructure scaffolding — sera-errors/cache/secrets/testing/session
- §Acceptance-test catalog — design-forward obligations (§4.6) traceability

---
## Sequencing, parallel lanes, and milestones

---

### 1. Dependency graph

```
                         ┌──► P0-2 sera-telemetry ────────────────────────────────────┐
                         │      (new crate alongside sera-events)                      │
                         │                                                             │
                         ├──► P0-3 sera-config ────────────────────────────────────────┤
                         │      (independent leaf, no cascade)                        │
                         │                                                             ▼
P0-1 sera-types ─────────┤                                              P0-5 sera-gateway ──► P0-6 sera-runtime
(rename-in-place,        │                                              (tightly coupled;      (depends on P0-5
 single first-mover)     ├──► P0-4 sera-queue ──────────────────────► both in same lane)      AppServerTransport
                         │      (extraction from sera-db)                             ▲        and P0-1 TurnOutcome)
                         │                                                             │
                         ├──► P0-8 sera-tools ──────────────────────────────────────► ┘
                         │      (new crate absorbing sera-docker;            (gateway re-imports
                         │       sera-docker shim kept until P0-5/P0-6       QueueBackend +
                         │       shim removal)                                SandboxProvider)
                         │
                         ├──► P0-7 sera-auth ──────────────────────────────────────────── (no runtime/gateway dep in P0)
                         │      (extension: argon2 + casbin + CapabilityToken)
                         │
                         └──► P0-9 sera-workflow ──────────────────────────────────────── (no runtime/gateway dep in P0)
                                (rewrite: types + claim protocol only)

P0-10 scaffolding (sera-errors, sera-cache, sera-secrets, sera-testing, sera-session)
  ├── sera-errors: unblocked (no deps); scaffold alongside P0-4/P0-8
  ├── sera-cache: unblocked; scaffold alongside P0-4/P0-8
  ├── sera-secrets: unblocked; scaffold alongside P0-8 (DockerSandboxProvider needs it)
  ├── sera-testing: after P0-4 + P0-8 (mock QueueBackend + mock SandboxProvider)
  └── sera-session: after P0-4 + P0-8 (6-state machine needs QueueBackend trait stable)

Shim removal dependency:
  P0-5 + P0-6 complete ──► sera-docker shim deleted (call sites migrated to DockerSandboxProvider)
```

**Critical path summary:**

- **sera-types is the sole first-mover and the hardest serialisation point.** Every other P0 item imports `sera-domain` under the old name; the mechanical rename + structural breaks (`TurnOutcome`, `SessionState` additions, `HookPoint::ALL` count) must be fully merged before any parallel lane can compile cleanly. No other work should begin until `cargo check -p sera-types` is green and the downstream breakage triage (sera-runtime match sites, sera-hooks count assertion, sera-core pipeline) is resolved.

- **Gateway and runtime are architecturally coupled and must be treated as a single lane.** P0-6 (sera-runtime) has two hard prerequisites: P0-1 (sera-types — `TurnOutcome`, `ContentBlock`, `ActionId`) and P0-5 (sera-gateway — `AppServerTransport::Stdio` for `main.rs` re-plumb). The `AgentHarness` trait that gateway consumes is defined in sera-types to avoid a circular dependency; if this cross-crate contract shifts, both crates break simultaneously. Lane D must keep a single agent accountable for the gateway/runtime pair.

- **Queue and tools absorption must land before the gateway structural rewrite is complete.** `sera-gateway/src/state.rs` acquires `queue_backend: Arc<dyn QueueBackend>` (from P0-4) and `DockerSandboxProvider` (from P0-8). The sera-docker shim keeps `sera-core/src/bin/sera.rs` compiling until both crates land and the call-site migration in orchestrator/cleanup/state/main is executed as part of P0-5. Lane D cannot be signed off until Lanes B and C have merged.

- **Telemetry, config, workflow, and auth are independent after sera-types lands.** None of these four items import from sera-gateway or sera-runtime in Phase 0. sera-workflow (P0-9) and sera-auth (P0-7) share only sera-types primitives (`BlastRadius`, `ChangeArtifactId`, `AgentCapability`) with each other — they do not depend on each other. These can proceed concurrently in Lane E immediately after M0 is reached.

- **The P0-10 scaffolding crates (sera-errors, sera-cache, sera-secrets) are unblocked from M0.** Only sera-testing and sera-session require Lane B/C outputs to be stable, since they mock `QueueBackend` and the 6-state `SessionState` machine respectively.

---

### 2. Parallel lanes

#### Lane A — types first-mover (serialised, blocks all other lanes)

| Property | Value |
|---|---|
| P0 items | P0-1 sera-domain → sera-types |
| Waits for | Nothing (first lane) |
| Blocks | All other lanes |
| Relative size | M — 15 acceptance tests, ~5 files modified, ~3 files created, mechanical rename across 13 `Cargo.toml` files |

This lane runs alone. The rename commit lands first (clean diff); structural additions (`evolution.rs`, `versioning.rs`, `content_block.rs`, enum additions) land in a second commit on the same branch. Gate: `cargo check --workspace` shows only expected downstream breakage at known sites; no new compilation errors introduced by the rename itself.

#### Lane B — infrastructure foundations (parallel after Lane A)

| Property | Value |
|---|---|
| P0 items | P0-2 sera-events → sera-telemetry (new crate), P0-3 sera-config (extension), P0-4 sera-db/sera-queue split |
| Waits for | Lane A (M0) |
| Blocks | Lane D (gateway needs `QueueBackend` trait stable) |
| Relative size | L — three crates, ~25 combined acceptance tests, P0-4 alone is ~8 new files + migration of 597 LoC |

These three items share no inter-dependencies and can be fanned out to three parallel agents. sera-telemetry creates a new crate; sera-config is an in-place extension; sera-queue is a new crate extracting from sera-db. All three can compile independently once sera-types is renamed.

#### Lane C — tools absorption (parallel after Lane A, independent of Lane B)

| Property | Value |
|---|---|
| P0 items | P0-8 sera-docker → sera-tools absorption; P0-10 partial scaffolds (sera-errors, sera-cache, sera-secrets) |
| Waits for | Lane A (M0) |
| Blocks | Lane D (gateway acquires `DockerSandboxProvider`; shim removal requires P0-5/P0-6 complete) |
| Relative size | L — sera-tools is the largest new crate in Phase 0 (~15 files, 15 acceptance tests); scaffolds are each S |

sera-tools has no dependency on sera-queue or sera-telemetry in Phase 0 — the `AuditHandle` field in `DockerSandboxProvider` uses a locally-defined `NoOpAuditHandle` stub until sera-telemetry lands. This independence makes Lane C safely parallelisable with Lane B.

#### Lane D — gateway and runtime spine (after Lanes B and C)

| Property | Value |
|---|---|
| P0 items | P0-5 sera-core → sera-gateway rename + structural additions; P0-6 sera-runtime contract migration |
| Waits for | Lanes B (QueueBackend trait) + C (DockerSandboxProvider / sera-tools) |
| Blocks | Lane F (scaffolding crates that depend on gateway) |
| Relative size | L — P0-5 is ~12 new files + 4 modified routes + orchestrator refactor; P0-6 is a full runtime rewrite (~10 new files, 15 tests, main.rs deleted and rewritten) |

Despite their size, P0-5 and P0-6 should be owned by a single agent or tightly coordinated pair. The `AgentHarness` trait definition (in sera-types), the `AppServerTransport::Stdio` feature (in sera-gateway), and `main.rs` rewrite (in sera-runtime) form a three-way contract that breaks if done in isolation. Rename commit lands first; SQ/EQ structural additions follow; runtime contract migration last.

#### Lane E — workflow and auth typing (parallel after Lane A, independent of Lanes B/C/D)

| Property | Value |
|---|---|
| P0 items | P0-9 sera-workflow rewrite (types + claim protocol); P0-7 sera-auth extension (argon2 + casbin + CapabilityToken) |
| Waits for | Lane A (M0) only |
| Blocks | Nothing in Phase 0 (gateway and runtime imports are Phase 1) |
| Relative size | M each — P0-9 is ~6 new files, 14 tests; P0-7 is ~2 new files + 3 modified files, 12 tests |

These two items share sera-types primitives but have zero imports from each other. They can be assigned to separate agents running concurrently with Lane B, Lane C, and Lane D. Lane E produces stable type contracts (`WorkflowTaskId` content-hash, `ClaimToken`, `CapabilityToken` narrowing) that later phases import — correctness of the types matters more than delivery order.

#### Lane F — scaffolding completion (after Lanes D and E)

| Property | Value |
|---|---|
| P0 items | P0-10 remainder: sera-testing (mock QueueBackend + mock SandboxProvider), sera-session (6-state machine + ContentBlock transcript) |
| Waits for | Lanes D + E (QueueBackend and SessionState extensions both stable) |
| Blocks | Phase 1 only |
| Relative size | S each — trait mocks and state machine scaffolds, no production logic |

sera-testing and sera-session are intentionally deferred to after the spine is stable. Mocking an unstable trait creates churn; scaffolding the session state machine before `SessionState` extensions from P0-1 are confirmed correct risks a second rewrite.

---

### 3. Milestone markers M0..M4

#### M0 — sera-types first-mover lands

**Goal:** Establish the single shared-types foundation that every downstream crate can import. Unblock all parallel lanes.

**Exit criteria:**
- `cargo check -p sera-types` passes with zero errors on default features.
- Directory `rust/crates/sera-types/` exists; `rust/crates/sera-domain/` is absent from the workspace members array.
- All 13 dependent `Cargo.toml` files updated to `sera-types.workspace = true`.
- Global `use sera_domain::` → `use sera_types::` search-replace complete; no remaining `sera_domain` references in `*.rs` files.
- All 15 acceptance tests from the P0-1 catalog compile and pass: evolution variants, serde roundtrips, session state arcs, content block tagging, `BuildIdentity` serde, `ResourceKind` parse.
- Known downstream breakage triaged and documented: `sera-runtime` `TurnResult` match sites, `sera-hooks` `HookPoint::ALL` count assertion (16 → 20), `sera-core` gateway pipeline pattern-match — all acknowledged with `// TODO(P0-5/P0-6)` stubs, not silently broken.

**Dependency gate:** Lane A complete.

**Next-session focus when M0 lands:** Fan out four parallel agents immediately — one each for Lanes B, C, and E (workflow/auth can split further), plus begin the Lane D rename commit which has minimal conflict surface at this point.

---

#### M1 — infrastructure in place

**Goal:** All foundation crates (telemetry, config, queue, tools) compile independently, ship their acceptance test suites, and expose stable trait surfaces that Lane D can build against.

**Exit criteria:**
- `cargo check -p sera-telemetry` green; OTel triad dependency pins present in `[workspace.dependencies]` with load-bearing version comments; `AuditBackend` trait object-safe; `LaneFailureClass` 15-variant enum with serde roundtrip.
- `cargo check -p sera-config` green; all design-forward config fields present per SPEC-config obligations.
- `cargo check -p sera-queue` green; `QueueBackend` trait object-safe; `LocalQueueBackend` push/pull/ack roundtrip; `GlobalThrottle` cap semantics; all 12 acceptance tests pass.
- `cargo check -p sera-tools` green; `SandboxProvider` trait object-safe; `SsrfValidator` blocks loopback/link-local/metadata; kill-switch CON-04 boot check; all 15 acceptance tests pass.
- P0-10 scaffolds (sera-errors, sera-cache, sera-secrets) added to workspace and `cargo check` clean; sera-errors exports `SeraErrorCode` used by `QueueError` and `SandboxError`.
- `sera-docker/Cargo.toml` has `publish = false` and the shim re-exports compile.
- `cargo check -p sera-queue --no-default-features` and `--features apalis` both pass.
- `cargo check -p sera-tools --no-default-features` and `--features docker` and `--features wasm` all pass.

**Dependency gate:** Lanes B and C complete.

**Next-session focus when M1 lands:** Begin Lane D. The gateway rename commit (P0-5 first commit) has minimal surface area — 3 files — and can land within the first hour of the next session, unblocking the full SQ/EQ structural sprint.

---

#### M2 — gateway and runtime spine

**Goal:** The SQ/EQ envelope round-trips end-to-end; `AppServerTransport` enum is exhaustive with `InProcess` always compiled; the four-method turn lifecycle skeleton runs under `DefaultHarness`; `main.rs` boots under `StdioTransport` and processes a NDJSON Submission.

**Exit criteria:**
- `cargo check -p sera-gateway` green on `default` features and `--features enterprise`.
- `Submission` / `Event` serde roundtrip tests pass; `Op` enum exhaustive match compiles.
- `AppServerTransport` enum present with all 6 variants; `InProcess` variant always compiled.
- `cargo check -p sera-runtime` green; `TurnResult` entirely absent from codebase (`grep -r TurnResult rust/` returns zero hits outside test tombstone comments).
- Four-method turn lifecycle (`_observe`, `_think`, `_act`, `_react`) callable in integration test with `DefaultHarness` + `NoOpCondenser` + mock LLM stub.
- Doom-loop threshold (`DOOM_LOOP_THRESHOLD = 3`) triggers `TurnOutcome::Interruption` in test.
- `MAX_COMPACTION_CHECKPOINTS_PER_SESSION = 25` constant present.
- `main.rs` subprocess test: spawn `sera-runtime` binary, send NDJSON `Submission { op: Op::UserTurn }`, receive at least one `Event` in response.
- Direct `reasoning_loop.rs`, `tool_loop_detector.rs`, `context_pipeline.rs`, `context_assembler.rs` deleted from sera-runtime; `TaskInput` / `TaskOutput` flat structs absent.
- sera-docker shim call-site migration complete: `sera-core/src/main.rs`, `state.rs`, `services/cleanup.rs`, `services/orchestrator.rs` all import from `sera_tools::sandbox::docker::DockerSandboxProvider`; `sera-docker` crate deleted.
- All 12 gateway acceptance tests and all 15 runtime acceptance tests pass.

**Dependency gate:** Lane D complete (which requires Lanes B and C at M1).

**Next-session focus when M2 lands:** Begin Phase 1 implementation work on the runtime execution substrate — sqlx persistence for `WorkflowTask`, apalis job workers, circle coordination, and HITL routing. Also wire `sera-testing` (Lane F) to enable integration tests across the full stack.

---

#### M3 — workflow and auth typed

**Goal:** The workflow type contract (`WorkflowTaskId` content-hash, `Hooked` atomic-claim state, termination triad) and the auth security baseline (argon2 key hashing, casbin RBAC, `CapabilityToken` narrowing) are both stable and tested. Design-forward obligations for the self-evolution pipeline are present as typed stubs.

**Exit criteria:**
- `cargo check -p sera-workflow` green; `WorkflowTaskId` is a `[u8;32]` SHA-256 content hash with stable Display/FromStr; `WorkflowTaskStatus::Hooked` present and documented as atomic-claim intermediate state; all 14 acceptance tests pass.
- `ready_tasks()` five-gate algorithm passes all 8 readiness tests including `ConditionalBlocks` edge cases.
- `claim_task()` compare-and-swap passes `atomic_claim_transitions_to_hooked` and `double_claim_returns_already_claimed` tests.
- `WorkflowTask.meta_scope: Option<BlastRadius>` and `.change_artifact_id: Option<ChangeArtifactId>` fields present and serde-stable.
- `cargo check -p sera-auth` green on `default` features, `--no-default-features`, and `--features enterprise`.
- `StoredApiKey.key_hash_argon2` PHC string in place; `grep -r 'key_hash ==' rust/crates/sera-auth/` returns zero hits (plaintext comparison path entirely absent).
- `CapabilityToken::narrow()` widening-attempt rejection tested; proposal-limit enforcement tested.
- `casbin::Enforcer` wired in `CasbinAuthzProvider`; RBAC allow/deny tests pass.
- `Action::ProposeChange` and `Action::ApproveChange` and `Resource::ChangeArtifact` present in `authz.rs`; default deny stubs in `DefaultAuthzProvider`.
- All 12 auth acceptance tests pass.

**Dependency gate:** Lane E complete (independent of Lane D; can reach M3 before or after M2).

**Next-session focus when M3 lands:** Wire `Action::ProposeChange`/`ApproveChange` into the gateway Submission authz path (P1 gateway task), and begin the HITL `ApprovalScope::ChangeArtifact` work using the stable `ChangeArtifactId` type from sera-types.

---

#### M4 — full Phase 0 gate

**Goal:** The entire workspace compiles clean across all feature matrix combinations; every acceptance test from the P0 catalog passes; all design-forward obligations from SPEC §4.6 are present as typed stubs; Phase 1 implementation work can begin without blockers.

**Exit criteria:**
- `cargo check --workspace` green.
- `cargo check --workspace --no-default-features` green.
- `cargo check --workspace --features enterprise` green.
- Zero remaining `TurnResult` references; zero remaining `sera_domain` references; zero remaining plaintext key comparison paths.
- All acceptance tests from the complete P0 catalog pass: 15 (P0-1) + sera-telemetry + sera-config + 12 (P0-4) + 15 (P0-8) + 12 (P0-5) + 15 (P0-6) + 14 (P0-9) + 12 (P0-7).
- Design-forward obligations checklist (SPEC §4.6) 100% present:
  - `ChangeArtifactId` in `TurnContext.change_artifact` (P0-6)
  - `BlastRadius` 22-variant enum non-exhaustive (P0-1)
  - `WorkflowTask.meta_scope` and `.change_artifact_id` (P0-9)
  - `AgentCapability` enum in sera-types, narrowing enforced in sera-auth (P0-7)
  - `HookPoint::ConstitutionalGate` present with `// TODO(P1)` stub at `_observe`/`_react` sites (P0-1, P0-6)
  - `BuildIdentity` in every `EventContext.generation` (P0-5)
- `sera-errors`, `sera-cache`, `sera-secrets`, `sera-testing` scaffolds added to workspace and compiling (P0-10).
- `sera-docker` crate deleted; no remaining references in workspace.
- `sera-events` crate deleted (or marked `publish = false` pending final migration); `sera-telemetry` is the sole observability crate.
- `WorkflowRegistry` marked `#[deprecated]` in favour of `WorkflowEngine` (Phase 1).
- CI passes on all three feature matrix configurations.

**Dependency gate:** All lanes (A through F) complete; M1, M2, M3 all reached.

**Next-session focus when M4 lands:** Begin Phase 1 — the runtime execution substrate. Priority order: (1) sqlx persistence for `WorkflowTask` and `ClaimToken` replacing in-memory store, (2) apalis job workers replacing `LocalQueueBackend` for production queue, (3) `HookPoint::ConstitutionalGate` enforcement wired into `_observe`/`_react` call sites, (4) HITL `ApprovalScope::ChangeArtifact` routing.

---

### 4. Context-budget note

Future ultrawork sessions executing this plan should fan out one sonnet agent per crate plan immediately after M0 is confirmed, following the same Wave 1 pattern used here: one agent owns one crate plan end-to-end (read the relevant agent file, implement, run `cargo check -p <crate>`, ship acceptance tests). Lane B's three crates (P0-2, P0-3, P0-4) and Lane C's crate (P0-8) are safe to fan out simultaneously — four parallel agents — since they share no compilation dependencies after sera-types lands. Use serena for symbol inspection within existing crates (particularly during the sera-runtime rewrite in Lane D, where old module deletions need reference checks) rather than loading whole files cold. Lane D should assign a single agent to both P0-5 and P0-6 given the tight `AgentHarness`/`AppServerTransport` coupling; splitting across two agents risks the three-way contract with sera-types drifting. Apply the 60% context-budget cap: agents reading the full Wave 1 plan files plus their target crate should not proceed to implementation if context is above 60% — spawn a fresh agent with only the relevant P0-N section and the crate source files needed.
## P0-1 · sera-domain → sera-types

---

### Rename strategy

**Decision: rename-in-place via `git mv`.**

Rationale: the crate has no internal split in purpose — it is and will remain the single shared-types leaf crate. Creating a new crate alongside would require a two-step deprecation cycle and a transient period where both names exist in the workspace, increasing the surface area of the P0 landing. HANDOFF §6.6 explicitly recommends rename-in-place. All consumers need only a mechanical `Cargo.toml` + `use sera_domain::` → `use sera_types::` search-replace; there are no symbol renames.

---

### Workspace `Cargo.toml` edits

**`rust/Cargo.toml`** — three locations:

1. `members` array: replace `"crates/sera-domain"` with `"crates/sera-types"`.
2. `[workspace.dependencies]`: rename key `sera-domain` → `sera-types` and update path:
   ```toml
   sera-types = { path = "crates/sera-types" }
   ```
3. Remove the old `sera-domain = { path = "crates/sera-domain" }` line.

**`rust/crates/sera-types/Cargo.toml`** (the renamed file):

```toml
[package]
name = "sera-types"          # ← changed from "sera-domain"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
uuid.workspace = true
time.workspace = true
thiserror.workspace = true
chrono = { workspace = true }
async-trait.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio = { workspace = true }
```

**Every dependent `Cargo.toml`** — change `sera-domain.workspace = true` → `sera-types.workspace = true`:

| File | Current line | New line |
|------|-------------|----------|
| `rust/crates/sera-auth/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-byoh-agent/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-config/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-core/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-db/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-docker/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-events/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-hitl/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-hooks/Cargo.toml` | `sera-domain = { workspace = true }` | `sera-types = { workspace = true }` |
| `rust/crates/sera-runtime/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-testing/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-tui/Cargo.toml` | `sera-domain.workspace = true` | `sera-types.workspace = true` |
| `rust/crates/sera-workflow/Cargo.toml` | `sera-domain = { workspace = true }` | `sera-types = { workspace = true }` |

After the `git mv` and `Cargo.toml` edits, a global search-replace across all Rust source files:
- `use sera_domain::` → `use sera_types::`
- `sera_domain::` (qualified paths) → `sera_types::`
- `extern crate sera_domain` → `extern crate sera_types` (if present)

---

### Files to create

#### `rust/crates/sera-types/src/evolution.rs`

New file. Contains all self-evolution Phase 0 primitives (SPEC-self-evolution §5.1, §9).

**Types:**

- `ChangeArtifactId { hash: [u8; 32] }` — content-addressed identity (SHA-256). Derives Debug/Clone/Copy/PartialEq/Eq/Hash/Serde. Display as hex.
- `BlastRadius` `#[non_exhaustive]` — 22-variant enum (AgentMemory, AgentPersonaMutable, AgentSkill, AgentExperiencePool, SingleHookConfig, SingleToolPolicy, SingleConnector, SingleCircleConfig, AgentManifest, TierPolicy, HookChainStructure, ApprovalPolicy, SecretProvider, GlobalConfig, RuntimeCrate, GatewayCore, ProtocolSchema, DbMigration, ConstitutionalRuleSet, KillSwitchProtocol, AuditLogBackend, SelfEvolutionPipeline).
- `CapabilityToken { id, scopes: HashSet<BlastRadius>, expires_at, max_proposals, signature: [u8;64] }`. Narrowing rule enforcement lives in sera-auth.
- `ConstitutionalRule { id, description, enforcement_point, content_hash }`.
- `ConstitutionalEnforcementPoint` — PreProposal/PreApproval/PreApplication/PostApplication.
- `EvolutionTier` `#[non_exhaustive]` — AgentImprovement/ConfigEvolution/CodeEvolution.
- `ChangeProposer { principal_id, capability_token }`.
- `AgentCapability` — MetaChange/CodeChange/MetaApprover/ConfigRead/ConfigPropose.

#### `rust/crates/sera-types/src/versioning.rs`

`BuildIdentity { version, commit: [u8;20], build_time, signer_fingerprint: [u8;32], constitution_hash: [u8;32] }` per SPEC-versioning §4.6.

#### `rust/crates/sera-types/src/content_block.rs`

- `ContentBlock` tagged enum (`#[serde(tag = "type")]`): `Text { text }`, `ToolUse { id, name, input }`, `ToolResult { tool_use_id, tool_name, output, error }`.
- `ConversationRole` — User/Assistant/System/Tool.
- `ConversationMessage { role, content: Vec<ContentBlock>, usage: Option<TokenUsage>, cause_by: Option<ActionId> }`. Invariant enforced by constructor: `role==Tool` ⇒ ≥1 ToolResult; `role==Assistant` may contain ToolUse.
- `ActionId(pub String)` newtype with From/AsRef/Display.

---

### Files to modify

#### `rust/crates/sera-types/src/runtime.rs`

Replace `TurnResult` struct with `TurnOutcome` enum (SPEC-runtime §2.3):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TurnOutcome {
    RunAgain { tool_calls, tokens_used, duration_ms },
    Handoff { target_agent_id, context, tokens_used, duration_ms },
    FinalOutput { response, tool_calls, tokens_used, duration_ms },
    Compact { tokens_used, duration_ms },
    Interruption { hook_point, reason, duration_ms },
    Stop { summary, tokens_used, duration_ms },
}
```

Update `AgentRuntime::execute_turn` return type. Add `TurnContext.change_artifact: Option<ChangeArtifactId>`.

#### `rust/crates/sera-types/src/session.rs`

Add `SessionState::Spawning`, `::TrustRequired`, `::ReadyForPrompt`, `::Paused`, `::Shadow` per SPEC-gateway §6.1 and SPEC-self-evolution §5.5. Update `can_transition_to` with new arcs: Created→Spawning, Spawning→Created|Destroyed, Active→TrustRequired, TrustRequired→Active|Archived, Active→ReadyForPrompt→Active, Active→Paused→Active|Archived|Destroyed, Created→Shadow→Destroyed. Update `is_runnable` to include ReadyForPrompt.

#### `rust/crates/sera-types/src/config_manifest.rs`

- `ResourceKind`: add `SandboxPolicy`, `Circle`, `ChangeArtifact` variants. Update FromStr/Display/match sites.
- `ResourceMetadata`: add `change_artifact: Option<ChangeArtifactId>`, `shadow: bool` (default false).
- `PersonaSpec`: add `mutable_persona: Option<String>`, `mutable_token_budget: Option<u32>`.

#### `rust/crates/sera-types/src/capability.rs`

Add `AgentCapability` enum (distinct from `ResolvedCapabilities` container grants):

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    MetaChange, CodeChange, MetaApprover, ConfigRead, ConfigPropose,
}
```

#### `rust/crates/sera-types/src/hook.rs`

- Add `HookPoint::ConstitutionalGate` (fail-closed), `::OnLlmStart`, `::OnLlmEnd`, `::OnChangeArtifactProposed`. Update `HookPoint::ALL` total to 20.
- Add `HookContext.change_artifact: Option<ChangeArtifactId>`.
- Add `updated_input: Option<serde_json::Value>` field to `HookResult::Continue`.

#### `rust/crates/sera-types/src/lib.rs`

```rust
pub mod evolution;
pub mod versioning;
pub mod content_block;
pub use evolution::*;
pub use versioning::BuildIdentity;
pub use content_block::{ContentBlock, ConversationMessage, ConversationRole};
```

---

### Cargo features

**None required.** Design-forward obligations must be unconditionally present; gating would defeat the purpose.

### `#[non_exhaustive]` additions (SPEC-versioning §5.2)

`ResourceKind`, `SessionState`, `BlastRadius`, `EvolutionTier`.

---

### Acceptance tests

| # | File | Test fn | Asserts |
|---|------|--------|---------|
| 1 | `tests/evolution.rs` | `blast_radius_has_22_variants` | Exhaustive match with `#[deny(unreachable_patterns)]` — 22 arms |
| 2 | `tests/evolution.rs` | `blast_radius_serde_roundtrip_all_variants` | JSON roundtrip all 22 |
| 3 | `tests/evolution.rs` | `change_artifact_id_display_is_hex` | `[0u8;32]` → 64-char lowercase hex |
| 4 | `tests/evolution.rs` | `capability_token_scope_is_set` | HashSet semantics |
| 5 | `tests/evolution.rs` | `evolution_tier_non_exhaustive_serde` | 3 variants roundtrip |
| 6 | `tests/evolution.rs` | `agent_capability_all_variants_serde` | 5 variants snake_case |
| 7 | `tests/content_block.rs` | `content_block_type_tag_in_json` | `{"type":"text","text":"hi"}` |
| 8 | `tests/content_block.rs` | `conversation_message_cause_by_roundtrip` | cause_by preserved |
| 9 | `tests/content_block.rs` | `conversation_message_tool_use_tool_result_pairing` | Pairing in `Vec<ContentBlock>` |
| 10 | `tests/session.rs` | `session_state_new_variants_serde` | 5 new variants serde |
| 11 | `tests/session.rs` | `shadow_session_valid_transitions` | Created→Shadow→Destroyed OK; Shadow→Active rejected |
| 12 | `tests/config_manifest.rs` | `resource_kind_has_sandbox_policy_circle_change_artifact` | FromStr parse |
| 13 | `tests/config_manifest.rs` | `resource_metadata_shadow_field_defaults_false` | Default false |
| 14 | `tests/config_manifest.rs` | `persona_spec_mutable_fields_round_trip` | YAML roundtrip |
| 15 | `tests/versioning.rs` | `build_identity_serde_roundtrip` | Full field preservation |

---

### Downstream cascade

`cargo check --workspace` will break every crate importing `sera-domain` (mechanical rename fix). Structural breaks requiring non-trivial fixes:
- **`sera-runtime`**: `AgentRuntime::execute_turn` signature changes `TurnResult` → `TurnOutcome`; `DefaultRuntime` match sites must be rewritten.
- **`sera-core`**: gateway pipeline pattern-matches turn result.
- **`sera-hooks`**: `HookPoint::ALL` len assertion changes 16 → 20; `hook_point_count` test must update.

`sera-hitl` is not directly broken but becomes blocked on `ApprovalScope::ChangeArtifact`/`MetaChange` additions (P1, downstream of `ChangeArtifactId` stabilising here).
I have all the information needed. Let me now write the two plan sections.

## P0-2 · sera-events → sera-telemetry

### Rename + rewrite strategy

**Decision: new crate alongside, then delete.**

`sera-events` is classified `needs-rewrite` in §2.3, not `needs-extension`. The conceptual mismatch is total: the existing crate is a Centrifugo sidecar (JWT generation + HTTP publish + `AuditHashChain` over string fields). The target is a full OTel/OCSF observability layer with a separate credential-isolated audit write path. These are not the same thing extended — they are different things that happen to share a crate name.

Rationale for new-crate-alongside:

1. **Dependency isolation**: `sera-events` today is consumed by `sera-docker` (via `CentrifugoClient`). Renaming in-place forces that caller to break at the same moment every new type lands. A parallel new crate lets `sera-telemetry` stabilize while `sera-docker` still compiles against the old surface until its own migration sprint.
2. **No `jsonwebtoken` contamination**: The `jsonwebtoken` dep must move to `sera-auth` (§4.7). A clean new crate starts with a correct dep graph. Rename-in-place risks the dep lingering via merge conflicts or oversight.
3. **Cargo crate name is the public identity**: `package.name = "sera-events"` would need to change to `"sera-telemetry"` anyway, which is effectively a new crate in the registry sense. Starting fresh is honest.
4. **Audit credential isolation**: §2.3 states "audit log writes must use credentials never exposed to the normal event pipeline." A new crate with no shared module surface enforces this at the type-system boundary from day one.

Migration sequence:
1. Create `rust/crates/sera-telemetry/` with all new content (this plan).
2. Add `sera-telemetry` to workspace `Cargo.toml` members.
3. Add OTel triad pins to `[workspace.dependencies]` with load-bearing comment.
4. Migrate `sera-docker`'s Centrifugo event emission to a thin adapter inside `sera-gateway` (P0-5 sprint).
5. Delete `rust/crates/sera-events/` once `sera-docker` and `sera-gateway` no longer reference it.

---

### Files to create

All paths are under `rust/crates/sera-telemetry/`.

**`Cargo.toml`**

```toml
[package]
name = "sera-telemetry"
version.workspace = true
edition.workspace = true

[dependencies]
sera-domain.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-opentelemetry.workspace = true
opentelemetry.workspace = true
opentelemetry-otlp.workspace = true
thiserror.workspace = true
sha2 = "0.10"
once_cell = "1"
time.workspace = true
uuid.workspace = true
async-trait.workspace = true

[features]
default = []
# Enable the in-process OTLP exporter backend (pulls in tonic/grpc)
otlp-exporter = ["opentelemetry-otlp/grpc-tonic"]

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-single-thread"] }
```

Note: `jsonwebtoken` is intentionally absent. JWT for Centrifugo belongs in `sera-auth` (§4.7). The `otlp-exporter` feature gates the heavy gRPC stack so `sera-runtime` (which runs inside containers) can depend on `sera-telemetry` without pulling in tonic.

---

**`src/lib.rs`**

```rust
//! sera-telemetry — OTel triad, OCSF v1.7.0 audit events, Emitter namespace tree.
//!
//! Replaces the legacy `sera-events` crate (Centrifugo sidecar) with a full
//! observability layer per SPEC-observability §2–§3 and SPEC-self-evolution §5.7.
//!
//! # Crate topology
//! - `audit`    — OCSF v1.7.0 `AuditEntry`, `AuditBackend` trait, `OnceCell` static binding
//! - `otel`     — pinned OTel triad initialisation helpers
//! - `emitter`  — hierarchical Emitter namespace tree (BeeAI pattern, §10.16)
//! - `lane_failure` — `LaneFailureClass` 15-variant typed failure taxonomy
//! - `generation`   — `GenerationMarker` / `GenerationLabel` / `BuildIdentity` (design-forward)
//! - `provenance`   — `LaneCommitProvenance`, `RunEvidence`, `CostRecord`

pub mod audit;
pub mod emitter;
pub mod generation;
pub mod lane_failure;
pub mod otel;
pub mod provenance;

// Flat re-exports for common call-sites.
pub use audit::{AuditBackend, AuditEntry, AUDIT_BACKEND};
pub use emitter::{Emitter, EventMeta};
pub use generation::{GenerationLabel, GenerationMarker};
pub use lane_failure::LaneFailureClass;
pub use otel::init_otel;
pub use provenance::{CostRecord, LaneCommitProvenance, RunEvidence};
```

---

**`src/audit.rs`**

The audit module enforces the credential-isolation invariant: `AuditBackend` has no delete/update surface, and the global binding is `OnceCell`-set-once (double-set panics).

```rust
//! OCSF v1.7.0 audit events and the isolated write path.
//!
//! # Invariants (SPEC-self-evolution §5.7)
//! 1. `AuditBackend` exposes ONLY `append` and `verify_chain` — no delete, no update.
//! 2. The `AUDIT_BACKEND` global is set once at boot via `set_audit_backend()`; a
//!    second call panics to prevent trust-collapse attacks via backend substitution.
//! 3. Audit writes MUST NOT share credentials or channels with the normal event bus.

use async_trait::async_trait;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// OCSF v1.7.0 audit event.
///
/// `ocsf_class_uid` taxonomy:
/// - `2004` — Detection Finding (used for `LaneFailureClass` events)
/// - `3001` — Account Change
/// - `6001` — File System Activity
/// - `6003` — API Activity (agent turn events)
///
/// See SPEC-observability §3.2 and NVIDIA OpenShell OCSF v1.7.0 schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// OCSF class UID (required field, v1.7.0 §2).
    pub ocsf_class_uid: u32,
    /// Serialised OCSF event payload (class-specific schema).
    pub payload: serde_json::Value,
    /// SHA-256 hash of the previous entry in the chain; `[0u8; 32]` for genesis.
    pub prev_hash: [u8; 32],
    /// SHA-256 hash of `(prev_hash ++ payload_bytes)` for this entry.
    pub this_hash: [u8; 32],
    /// Optional detached signature over `this_hash` (Phase 2 obligation; `None` in Phase 0).
    pub signature: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit backend not initialised — call set_audit_backend() at boot")]
    NotInitialised,
    #[error("hash chain verification failed at entry {index}: expected {expected}, got {got}")]
    ChainBroken {
        index: usize,
        expected: String,
        got: String,
    },
    #[error("backend write error: {0}")]
    Write(String),
}

/// The sole interface for writing audit events.
///
/// Implementors MUST:
/// - Append only — no `delete`, no `update` method exists here or on any
///   wrapper type (SPEC-self-evolution §5.7).
/// - Use credentials that are never exposed to the normal event bus.
#[async_trait]
pub trait AuditBackend: Send + Sync + 'static {
    /// Append a single audit entry. Returns the stored entry with `this_hash` populated.
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError>;

    /// Walk the stored chain and verify hash linkage. Returns the entry count on success.
    async fn verify_chain(&self) -> Result<usize, AuditError>;
}

/// Process-global audit backend binding.
///
/// Set once at boot via `set_audit_backend()`. Reading before it is set returns `None`;
/// callers should treat a missing backend as a hard startup fault.
pub static AUDIT_BACKEND: OnceCell<&'static dyn AuditBackend> = OnceCell::new();

/// Register the process-wide audit backend. Panics if called more than once.
///
/// Call this early in `main()`, before any component that might emit audit events.
/// The panic on double-set is intentional: it closes the trust-collapse attack class
/// described in SPEC-self-evolution §5.7 (an attacker substituting a no-op backend).
pub fn set_audit_backend(backend: &'static dyn AuditBackend) {
    AUDIT_BACKEND
        .set(backend)
        .expect("set_audit_backend() called twice — audit backend must be set exactly once at boot");
}

/// Append an audit entry using the global backend. Returns `Err(NotInitialised)` if
/// `set_audit_backend()` has not been called yet.
pub async fn audit_append(entry: AuditEntry) -> Result<AuditEntry, AuditError> {
    let backend = AUDIT_BACKEND.get().ok_or(AuditError::NotInitialised)?;
    backend.append(entry).await
}
```

---

**`src/otel.rs`**

```rust
//! Pinned OTel triad initialisation helpers.
//!
//! The three crates MUST move together (HANDOFF §6.2, SPEC-dependencies §8.4):
//!   opentelemetry        = "=0.27"   # EXACT — load-bearing triad (see SPEC-dependencies §8.4)
//!   opentelemetry-otlp   = "=0.27"   # EXACT
//!   tracing-opentelemetry = "=0.28"  # EXACT
//! Drift produces compile-time trait-bound errors. Never change one without the others.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

#[derive(Debug, thiserror::Error)]
pub enum OtelInitError {
    #[error("OTel tracer init failed: {0}")]
    TracerInit(String),
    #[error("tracing subscriber install failed: {0}")]
    SubscriberInstall(String),
}

/// Initialise the OTel triad and install the global tracing subscriber.
///
/// `endpoint` — OTLP gRPC endpoint, e.g. `"http://otel-collector:4317"`.
/// No-ops if `OTEL_SDK_DISABLED=true` is set in the environment.
///
/// Feature-gated: only available with `--features otlp-exporter`.
#[cfg(feature = "otlp-exporter")]
pub fn init_otel(service_name: &str, endpoint: &str) -> Result<(), OtelInitError> {
    use opentelemetry_otlp::TonicExporterBuilder;

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry::sdk::trace::Config::default().with_resource(
                opentelemetry::sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    service_name.to_owned(),
                )]),
            ),
        )
        .install_batch(opentelemetry::runtime::Tokio)
        .map_err(|e| OtelInitError::TracerInit(e.to_string()))?;

    let tracer = tracer_provider.tracer(service_name.to_owned());
    let otel_layer = OpenTelemetryLayer::new(tracer);

    let subscriber = Registry::default().with(otel_layer);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| OtelInitError::SubscriberInstall(e.to_string()))?;

    Ok(())
}

/// Stub when `otlp-exporter` feature is not enabled.
#[cfg(not(feature = "otlp-exporter"))]
pub fn init_otel(_service_name: &str, _endpoint: &str) -> Result<(), OtelInitError> {
    Ok(())
}
```

---

**`src/emitter.rs`**

```rust
//! Hierarchical Emitter namespace tree (BeeAI pattern, SPEC-observability §10.16).
//!
//! An `Emitter` represents a named scope in the event namespace. Emitters form a
//! tree: a root emitter spawns child emitters via `child()`. Each child inherits
//! the parent's namespace prefix and trace context, then appends its own segment.
//!
//! Example tree:
//!   sera.session.abc123
//!     └─ sera.session.abc123.turn.1
//!         └─ sera.session.abc123.turn.1.tool.shell

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

/// A point in the Emitter namespace tree.
///
/// Cloning an `Emitter` is cheap (inner `Arc`). The root singleton is obtained via
/// `Emitter::root()`. Child emitters are created via `emitter.child("segment")`.
#[derive(Debug, Clone)]
pub struct Emitter {
    inner: Arc<EmitterInner>,
}

#[derive(Debug)]
struct EmitterInner {
    /// Dotted namespace path, e.g. `"sera.session.abc123.turn.1"`.
    namespace: String,
    /// W3C `traceparent` string for this scope; propagated to child emitters.
    trace: Option<String>,
    /// Parent emitter, if any.
    parent: Option<Emitter>,
}

impl Emitter {
    /// Return the process-global root emitter (`"sera"`).
    pub fn root() -> Self {
        Self {
            inner: Arc::new(EmitterInner {
                namespace: "sera".to_owned(),
                trace: None,
                parent: None,
            }),
        }
    }

    /// Create a child emitter with the given namespace segment appended.
    ///
    /// `segment` must be a single dotless identifier (e.g. `"session"`, `"turn"`).
    /// Panics in debug builds if `segment` contains a `.`.
    pub fn child(&self, segment: &str) -> Self {
        debug_assert!(!segment.contains('.'), "Emitter segment must not contain '.'");
        Self {
            inner: Arc::new(EmitterInner {
                namespace: format!("{}.{}", self.inner.namespace, segment),
                trace: self.inner.trace.clone(),
                parent: Some(self.clone()),
            }),
        }
    }

    /// Return the full dotted namespace path.
    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    /// Attach a W3C traceparent to this emitter (returns a new emitter with the trace set).
    pub fn with_trace(self, traceparent: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(EmitterInner {
                namespace: self.inner.namespace.clone(),
                trace: Some(traceparent.into()),
                parent: self.inner.parent.clone(),
            }),
        }
    }

    /// Build an `EventMeta` for an event emitted from this scope.
    pub fn event_meta(&self, name: impl Into<String>, data_type: impl Into<String>) -> EventMeta {
        EventMeta {
            id: Uuid::new_v4(),
            name: name.into(),
            path: self.inner.namespace.clone(),
            created_at: OffsetDateTime::now_utc(),
            trace: self.inner.trace.clone(),
            data_type: data_type.into(),
        }
    }
}

/// Metadata attached to every event emitted through the Emitter tree.
///
/// Per SPEC-observability §2.1a. Fields `source`, `creator`, `context`, `group_id`
/// are Phase 1 obligations and omitted from the Phase 0 struct to avoid
/// introducing unresolved type dependencies on `sera-domain` changes not yet landed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    pub id: Uuid,
    pub name: String,
    /// Emitter namespace path at time of emission.
    pub path: String,
    pub created_at: OffsetDateTime,
    /// W3C traceparent, if any.
    pub trace: Option<String>,
    /// Logical data type discriminant for deserialisation routing.
    pub data_type: String,
}
```

---

**`src/lane_failure.rs`**

```rust
//! Typed lane failure taxonomy (SPEC-observability §3.3, SPEC-dependencies §10.1).
//!
//! Every OCSF `2004 Detection Finding` event carries a `sera.lane_failure_class`
//! extension field. Using a typed enum (rather than error strings) lets the
//! coordinator agent route, aggregate, and escalate failures without parsing text.

use serde::{Deserialize, Serialize};

/// 15-variant typed failure taxonomy for SERA lanes.
///
/// Variants are intentionally exhaustive at Phase 0. Adding a new variant
/// is a breaking change to any `match` statement — this is by design:
/// it forces callers to consciously handle new failure modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LaneFailureClass {
    /// Model API returned no response or timed out.
    PromptDelivery,
    /// Trust gate rejected the principal (capability check failed).
    TrustGate,
    /// Working branch has diverged from the canonical commit.
    BranchDivergence,
    /// Compilation step failed (Rust/TypeScript/other).
    Compile,
    /// Test suite failed.
    Test,
    /// Plugin process failed to start.
    PluginStartup,
    /// MCP server failed to start.
    McpStartup,
    /// MCP protocol handshake failed.
    McpHandshake,
    /// Gateway failed to route the submission.
    GatewayRouting,
    /// Tool invocation failed at runtime.
    ToolRuntime,
    /// Agent's worktree does not match expected workspace.
    WorkspaceMismatch,
    /// Infrastructure failure (container OOM, network partition, etc.).
    Infra,
    /// Orphaned run reaped by the watchdog.
    OrphanReaped,
    /// Constitutional gate rejected a proposed change (SPEC-self-evolution §5.7).
    ConstitutionalViolation,
    /// Kill switch activated by operator or automated policy (SPEC-self-evolution §5.7).
    KillSwitchActivated,
}

impl LaneFailureClass {
    /// Returns the OCSF `sera.lane_failure_class` extension field value.
    pub fn as_ocsf_extension(&self) -> &'static str {
        match self {
            Self::PromptDelivery => "prompt_delivery",
            Self::TrustGate => "trust_gate",
            Self::BranchDivergence => "branch_divergence",
            Self::Compile => "compile",
            Self::Test => "test",
            Self::PluginStartup => "plugin_startup",
            Self::McpStartup => "mcp_startup",
            Self::McpHandshake => "mcp_handshake",
            Self::GatewayRouting => "gateway_routing",
            Self::ToolRuntime => "tool_runtime",
            Self::WorkspaceMismatch => "workspace_mismatch",
            Self::Infra => "infra",
            Self::OrphanReaped => "orphan_reaped",
            Self::ConstitutionalViolation => "constitutional_violation",
            Self::KillSwitchActivated => "kill_switch_activated",
        }
    }
}
```

---

**`src/generation.rs`**

```rust
//! GenerationMarker — design-forward Phase 0 obligation (SPEC-self-evolution §5.5).
//!
//! `GenerationMarker` must exist on `EventContext` in Phase 0 even though it is not
//! functionally interpreted until Phase 2. It allows the audit trail to record
//! which binary generation emitted each event, enabling retrospective audits
//! after a self-evolution cycle.
//!
//! `BuildIdentity` is the canonical type from SPEC-versioning §4.6 (also required
//! in `sera-domain` — this is a local mirror for telemetry use; the authoritative
//! definition will live in `sera-domain` once the P0-1 sera-types sprint lands).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Discriminant label for a generation epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationLabel(pub String);

/// Cryptographic identity of a SERA binary build (SPEC-versioning §4.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildIdentity {
    pub version: String,
    pub commit: String,
    pub build_time: OffsetDateTime,
    /// Fingerprint of the signing key, if the binary is signed.
    pub signer_fingerprint: Option<String>,
    /// SHA-256 of the constitutional ruleset compiled into this binary.
    pub constitution_hash: [u8; 32],
}

/// Attached to `EventContext` to record which generation emitted an event.
///
/// Phase 0: populated at boot from build-time constants and stored on the context.
/// Phase 2+: used by self-evolution audit queries to attribute events to
///   specific binary generations across an upgrade boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMarker {
    pub label: GenerationLabel,
    pub binary_identity: BuildIdentity,
    pub started_at: OffsetDateTime,
}
```

---

**`src/provenance.rs`**

```rust
//! LaneCommitProvenance, RunEvidence, CostRecord (SPEC-observability §3.1, §3.4).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// A git SHA stored as a hex string.
pub type GitSha = String;

/// Describes what a subagent actually did at the filesystem/git level.
///
/// Emitted at lane completion so the parent session can verify the durable record
/// rather than trusting a free-text summary (SPEC-observability §3.4).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaneCommitProvenance {
    pub commit: Option<GitSha>,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub canonical_commit: Option<GitSha>,
    pub superseded_by: Option<GitSha>,
    pub lineage: Vec<GitSha>,
}

/// Token cost record for a single model call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostRecord {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Cost in USD micro-cents (multiply by 1e-8 for USD).
    pub cost_micro_usd: u64,
}

/// Proof bundle left behind by a production run (SPEC-observability §3.1).
///
/// Enables operators to fully explain any run after the fact.
/// `ProofBundle` in the legacy `sera-domain::observability` partially overlaps
/// but is missing most fields — this is the authoritative replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvidence {
    pub run_id: Uuid,
    /// Tool names exposed to the agent during this run.
    pub tools_exposed: Vec<String>,
    /// Tool names actually invoked.
    pub tools_called: Vec<String>,
    /// IDs of HITL approval events within this run.
    pub approvals: Vec<Uuid>,
    /// Count of durable memory writes.
    pub memory_writes: u32,
    /// Individual cost records, one per model call.
    pub model_calls: Vec<CostRecord>,
    /// Aggregate cost for the run.
    pub total_cost: CostRecord,
    /// Terminal outcome label (mirrors `TurnOutcome` discriminant, stored as string
    /// to avoid a hard dep on `sera-domain` before the P0-1 rename lands).
    pub outcome: String,
}
```

---

**`tests/telemetry_audit.rs`**

```rust
//! Acceptance tests for sera-telemetry audit path.

use sera_telemetry::audit::{
    AuditBackend, AuditEntry, AuditError, set_audit_backend, AUDIT_BACKEND,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Minimal in-memory backend for tests
// ---------------------------------------------------------------------------

struct MemAudit {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemAudit {
    fn new() -> &'static Self {
        Box::leak(Box::new(Self {
            entries: Mutex::new(vec![]),
        }))
    }
}

#[async_trait]
impl AuditBackend for MemAudit {
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(entry)
    }

    async fn verify_chain(&self) -> Result<usize, AuditError> {
        let entries = self.entries.lock().unwrap();
        // Minimal: just return entry count for Phase 0
        Ok(entries.len())
    }
}
```

**`tests/test_audit_set_once.rs`**

```rust
//! (a) AuditBackend static binding is set-once: setting twice panics.
//!
//! This test intentionally uses `#[should_panic]` — the panic is the security
//! invariant (SPEC-self-evolution §5.7). Do not remove.

use sera_telemetry::audit::{AuditBackend, AuditEntry, AuditError, set_audit_backend};
use async_trait::async_trait;

struct NoopAudit;

#[async_trait]
impl AuditBackend for NoopAudit {
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> { Ok(entry) }
    async fn verify_chain(&self) -> Result<usize, AuditError> { Ok(0) }
}

static NOOP_A: NoopAudit = NoopAudit;
static NOOP_B: NoopAudit = NoopAudit;

#[test]
#[should_panic(expected = "set_audit_backend() called twice")]
fn audit_backend_set_twice_panics() {
    // Each integration test binary gets a fresh process, so OnceCell is empty.
    set_audit_backend(&NOOP_A);
    set_audit_backend(&NOOP_B); // must panic
}
```

**`tests/test_audit_ocsf_fields.rs`**

```rust
//! (b) AuditEntry schema matches OCSF v1.7.0 required fields.

use sera_telemetry::audit::AuditEntry;
use serde_json::json;

#[test]
fn audit_entry_has_ocsf_required_fields() {
    let entry = AuditEntry {
        ocsf_class_uid: 2004,
        payload: json!({ "message": "test finding" }),
        prev_hash: [0u8; 32],
        this_hash: [1u8; 32],
        signature: None,
    };

    // ocsf_class_uid must be present and serialisable
    let v = serde_json::to_value(&entry).unwrap();
    assert!(v.get("ocsf_class_uid").is_some());
    assert_eq!(v["ocsf_class_uid"], 2004u32);
    assert!(v.get("prev_hash").is_some());
    assert!(v.get("this_hash").is_some());
    assert!(v.get("payload").is_some());
}

#[test]
fn audit_entry_class_uid_2004_is_detection_finding() {
    // OCSF class 2004 is "Detection Finding" — the class used for LaneFailureClass events.
    let entry = AuditEntry {
        ocsf_class_uid: 2004,
        payload: serde_json::json!({}),
        prev_hash: [0u8; 32],
        this_hash: [0u8; 32],
        signature: None,
    };
    assert_eq!(entry.ocsf_class_uid, 2004);
}
```

**`tests/test_audit_write_path_isolation.rs`**

```rust
//! (c) Audit write path is separate from the main event bus.
//!
//! The isolation is structural: `audit_append()` goes through `AUDIT_BACKEND`
//! (a dedicated OnceCell); there is no shared channel or credential with the
//! Emitter tree. This test verifies the call path compiles and routes correctly.

use sera_telemetry::audit::{audit_append, AuditEntry, AuditError};

#[tokio::test]
async fn audit_append_returns_not_initialised_when_backend_unset() {
    // In a fresh test binary, AUDIT_BACKEND is unset.
    let entry = AuditEntry {
        ocsf_class_uid: 6003,
        payload: serde_json::json!({ "op": "api_activity" }),
        prev_hash: [0u8; 32],
        this_hash: [0u8; 32],
        signature: None,
    };
    let result = audit_append(entry).await;
    assert!(matches!(result, Err(AuditError::NotInitialised)));
}
```

**`tests/test_generation_marker.rs`**

```rust
//! (d) GenerationMarker on EventContext — design-forward Phase 0 obligation.

use sera_telemetry::generation::{BuildIdentity, GenerationLabel, GenerationMarker};
use time::OffsetDateTime;

#[test]
fn generation_marker_round_trips_json() {
    let marker = GenerationMarker {
        label: GenerationLabel("v2.0.0-alpha".to_owned()),
        binary_identity: BuildIdentity {
            version: "0.1.0".to_owned(),
            commit: "abc1234".to_owned(),
            build_time: OffsetDateTime::UNIX_EPOCH,
            signer_fingerprint: None,
            constitution_hash: [0u8; 32],
        },
        started_at: OffsetDateTime::UNIX_EPOCH,
    };

    let json = serde_json::to_string(&marker).unwrap();
    let decoded: GenerationMarker = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.label.0, "v2.0.0-alpha");
    assert_eq!(decoded.binary_identity.commit, "abc1234");
}

#[test]
fn generation_label_is_non_empty() {
    let label = GenerationLabel("sera-2.0".to_owned());
    assert!(!label.0.is_empty());
}
```

**`tests/test_lane_failure_exhaustive.rs`**

```rust
//! (e) LaneFailureClass is exhaustive at 15 variants.

use sera_telemetry::lane_failure::LaneFailureClass;

const ALL_VARIANTS: &[LaneFailureClass] = &[
    LaneFailureClass::PromptDelivery,
    LaneFailureClass::TrustGate,
    LaneFailureClass::BranchDivergence,
    LaneFailureClass::Compile,
    LaneFailureClass::Test,
    LaneFailureClass::PluginStartup,
    LaneFailureClass::McpStartup,
    LaneFailureClass::McpHandshake,
    LaneFailureClass::GatewayRouting,
    LaneFailureClass::ToolRuntime,
    LaneFailureClass::WorkspaceMismatch,
    LaneFailureClass::Infra,
    LaneFailureClass::OrphanReaped,
    LaneFailureClass::ConstitutionalViolation,
    LaneFailureClass::KillSwitchActivated,
];

#[test]
fn lane_failure_has_15_variants() {
    assert_eq!(ALL_VARIANTS.len(), 15, "LaneFailureClass must have exactly 15 variants per §3.3");
}

#[test]
fn constitutional_violation_is_present() {
    assert!(ALL_VARIANTS.contains(&LaneFailureClass::ConstitutionalViolation));
}

#[test]
fn kill_switch_activated_is_present() {
    assert!(ALL_VARIANTS.contains(&LaneFailureClass::KillSwitchActivated));
}

#[test]
fn all_variants_have_ocsf_extension_strings() {
    for v in ALL_VARIANTS {
        let ext = v.as_ocsf_extension();
        assert!(!ext.is_empty(), "variant {v:?} has empty OCSF extension string");
        assert!(!ext.contains(' '), "OCSF extension '{ext}' must use underscores, not spaces");
    }
}

#[test]
fn lane_failure_round_trips_json() {
    for v in ALL_VARIANTS {
        let json = serde_json::to_string(v).unwrap();
        let decoded: LaneFailureClass = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, v);
    }
}

#[test]
fn detection_finding_ocsf_extension_matches_constitutional_violation() {
    assert_eq!(
        LaneFailureClass::ConstitutionalViolation.as_ocsf_extension(),
        "constitutional_violation"
    );
}
```

**`tests/test_emitter_tree.rs`**

```rust
//! Additional tests: Emitter tree, LaneCommitProvenance, EventMeta.

use sera_telemetry::emitter::Emitter;
use sera_telemetry::provenance::LaneCommitProvenance;

#[test]
fn emitter_root_namespace_is_sera() {
    let root = Emitter::root();
    assert_eq!(root.namespace(), "sera");
}

#[test]
fn emitter_child_appends_segment() {
    let root = Emitter::root();
    let session = root.child("session");
    assert_eq!(session.namespace(), "sera.session");
    let turn = session.child("turn");
    assert_eq!(turn.namespace(), "sera.session.turn");
}

#[test]
fn emitter_event_meta_has_correct_path() {
    let emitter = Emitter::root().child("session").child("abc123");
    let meta = emitter.event_meta("turn_start", "TurnStartEvent");
    assert_eq!(meta.path, "sera.session.abc123");
    assert_eq!(meta.name, "turn_start");
    assert_eq!(meta.data_type, "TurnStartEvent");
}

#[test]
fn lane_commit_provenance_default_is_all_none() {
    let p = LaneCommitProvenance::default();
    assert!(p.commit.is_none());
    assert!(p.branch.is_none());
    assert!(p.lineage.is_empty());
}
```

---

### Files to modify / delete from current `sera-events`

**Drop entirely** (after `sera-gateway` absorbs the Centrifugo adapter):

| File | Reason |
|---|---|
| `rust/crates/sera-events/src/centrifugo.rs` | Centrifugo is an infrastructure concern; moves to `sera-gateway` as a thin adapter |
| `rust/crates/sera-events/src/channels.rs` | Channel namespace logic couples to Centrifugo; moves with it |
| `rust/crates/sera-events/src/audit.rs` | Replaced by `sera-telemetry::audit` with OCSF-correct types |
| `rust/crates/sera-events/src/error.rs` | `CentrifugoError` / `AuditVerifyError` no longer needed here |
| `rust/crates/sera-events/src/lib.rs` | Entire crate is superseded |
| `rust/crates/sera-events/Cargo.toml` | Remove crate from workspace after callers migrate |

**The `jsonwebtoken` dep** (`sera-events/Cargo.toml` line 16) must not follow the migration. It belongs in `sera-auth`. When `sera-docker`'s Centrifugo integration moves to `sera-gateway`, `sera-auth` gains `jsonwebtoken` and calls through there.

**Migration gate**: delete `sera-events` only after `cargo check --workspace` passes with `sera-events` removed from `rust/Cargo.toml` `members`. Do not delete before P0-5 (`sera-gateway` sprint) lands.

---

### Cargo features

| Feature | Default | Purpose |
|---|---|---|
| `default` | yes (empty) | Core types only — no heavy OTel stack pulled in |
| `otlp-exporter` | opt-in | Enables `init_otel()` with tonic/gRPC OTLP exporter; pulled in by `sera-gateway` and `sera-core` binaries |

The `otlp-exporter` feature is opt-in because `sera-runtime` (which runs inside agent containers) needs `sera-telemetry` for `LaneFailureClass` and `AuditBackend` types but does not want to pull in `tonic` + `grpc` into a minimal container image.

---

### Workspace `Cargo.toml` dependency additions

Add to `[workspace.dependencies]` in `rust/Cargo.toml`:

```toml
# OTel triad — EXACT version pins are load-bearing.
# These three crates must move together; drift produces compile-time trait-bound errors.
# See SPEC-dependencies §8.4 and HANDOFF §6.2 before changing any of these pins.
opentelemetry        = "=0.27"   # EXACT — load-bearing triad
opentelemetry-otlp   = "=0.27"   # EXACT
tracing-opentelemetry = "=0.28"  # EXACT

once_cell = "1"
async-trait = "0.1"
```

The comment block (first three lines) is the "load-bearing comment" required by HANDOFF §6.2. The comment must survive any Cargo.toml reformatting — `cargo fmt` does not touch TOML comments.

---

### Acceptance tests summary

| File | Function | Assertion |
|---|---|---|
| `tests/test_audit_set_once.rs` | `audit_backend_set_twice_panics` | (a) Double-set of `AUDIT_BACKEND` panics with the expected message |
| `tests/test_audit_ocsf_fields.rs` | `audit_entry_has_ocsf_required_fields` | (b) Serialised `AuditEntry` contains `ocsf_class_uid`, `prev_hash`, `this_hash`, `payload` |
| `tests/test_audit_ocsf_fields.rs` | `audit_entry_class_uid_2004_is_detection_finding` | (b) Class UID 2004 constant round-trips |
| `tests/test_audit_write_path_isolation.rs` | `audit_append_returns_not_initialised_when_backend_unset` | (c) `audit_append()` is structurally separate from `Emitter`; returns `NotInitialised` without panicking |
| `tests/test_generation_marker.rs` | `generation_marker_round_trips_json` | (d) `GenerationMarker` serialises and deserialises cleanly |
| `tests/test_generation_marker.rs` | `generation_label_is_non_empty` | (d) Label type holds non-empty string |
| `tests/test_lane_failure_exhaustive.rs` | `lane_failure_has_15_variants` | (e) Exactly 15 variants |
| `tests/test_lane_failure_exhaustive.rs` | `constitutional_violation_is_present` | (e) Required security variant present |
| `tests/test_lane_failure_exhaustive.rs` | `kill_switch_activated_is_present` | (e) Required security variant present |
| `tests/test_lane_failure_exhaustive.rs` | `all_variants_have_ocsf_extension_strings` | (e) No blank/spaced extension strings |
| `tests/test_lane_failure_exhaustive.rs` | `lane_failure_round_trips_json` | (e) Full serde round-trip for all 15 variants |
| `tests/test_emitter_tree.rs` | `emitter_root_namespace_is_sera` | Emitter root is `"sera"` |
| `tests/test_emitter_tree.rs` | `emitter_child_appends_segment` | Child namespace = parent + `.` + segment |
| `tests/test_emitter_tree.rs` | `emitter_event_meta_has_correct_path` | `EventMeta.path` matches emitter namespace |

---

### Downstream cascade

Every crate that currently emits events or needs failure classification gains a `sera-telemetry` dep. The following crates will need `sera-telemetry.workspace = true` added to their `[dependencies]` in later sprints:

| Crate | What it needs from `sera-telemetry` |
|---|---|
| `sera-gateway` (renamed from `sera-core`) | `Emitter`, `AuditBackend` static init, `init_otel`, Centrifugo adapter replacement |
| `sera-runtime` | `LaneFailureClass`, `RunEvidence`, `GenerationMarker` on `EventContext` |
| `sera-hooks` | `LaneFailureClass::ConstitutionalViolation` for constitutional gate rejection events |
| `sera-hitl` | `AuditEntry` (HITL approvals must be audited), `LaneFailureClass::TrustGate` |
| `sera-docker` / `sera-tools` | `Emitter` for container lifecycle events (replaces `CentrifugoClient` calls) |
| `sera-workflow` | `RunEvidence` on task completion, `LaneFailureClass` on failure |
| `sera-auth` | `AuditEntry` for auth events (class 3001 Account Change) |

`sera-domain` does NOT get a `sera-telemetry` dep — domain types must remain leaf-crate with no observability stack pulled in.

---

## P0-3 · sera-config extensions

### Files to create

All paths are under `rust/crates/sera-config/src/`.

**`src/schema_registry.rs`**

```rust
//! Schema registry — stores and validates JSON Schemas generated from Rust types
//! via `schemars::schema_for!()` (SPEC-config §4, §4.1).
//!
//! Phase 0 scope: registration, retrieval, validation.
//! Phase 1 obligation: emit 13 JSON Schema files to `docs/schemas/` and add a
//! CI check verifying that the generated schemas stay in sync with the Rust types
//! (HANDOFF P1 item 4, IMPL-AUDIT §2.2).

use jsonschema::JSONSchema;
use schemars::schema::RootSchema;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

use sera_domain::config_manifest::ResourceKind;

/// API version string, e.g. `"sera.io/v1"`.
pub type ApiVersion = String;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("no schema registered for ({kind:?}, {api_version})")]
    NotFound {
        kind: ResourceKind,
        api_version: ApiVersion,
    },
    #[error("schema compilation failed: {0}")]
    Compile(String),
    #[error("validation failed: {errors:?}")]
    Invalid { errors: Vec<String> },
}

/// Registry of JSON Schemas keyed by `(ResourceKind, ApiVersion)`.
///
/// Populated at boot by calling `register()` once per managed resource type.
/// In Phase 1, a CI test verifies that the registered schemas match the
/// files in `docs/schemas/`.
#[derive(Default)]
pub struct SchemaRegistry {
    schemas: HashMap<(ResourceKind, ApiVersion), RootSchema>,
    compiled: HashMap<(ResourceKind, ApiVersion), JSONSchema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a schema generated via `schemars::schema_for!(T)`.
    ///
    /// The `compiled` entry is built eagerly so that `validate()` is allocation-free
    /// at call time.
    pub fn register(
        &mut self,
        kind: ResourceKind,
        api_version: impl Into<ApiVersion>,
        schema: RootSchema,
    ) -> Result<(), SchemaError> {
        let key = (kind, api_version.into());
        let schema_value = serde_json::to_value(&schema)
            .map_err(|e| SchemaError::Compile(e.to_string()))?;
        let compiled = JSONSchema::compile(&schema_value)
            .map_err(|e| SchemaError::Compile(e.to_string()))?;
        self.schemas.insert(key.clone(), schema);
        self.compiled.insert(key, compiled);
        Ok(())
    }

    /// Retrieve the raw `RootSchema` for a given `(kind, api_version)`.
    pub fn get_schema(
        &self,
        kind: &ResourceKind,
        api_version: &str,
    ) -> Result<&RootSchema, SchemaError> {
        self.schemas
            .get(&(kind.clone(), api_version.to_owned()))
            .ok_or_else(|| SchemaError::NotFound {
                kind: kind.clone(),
                api_version: api_version.to_owned(),
            })
    }

    /// Validate a JSON `Value` against the registered schema.
    pub fn validate(
        &self,
        kind: &ResourceKind,
        api_version: &str,
        value: &Value,
    ) -> Result<(), SchemaError> {
        let key = (kind.clone(), api_version.to_owned());
        let compiled = self.compiled.get(&key).ok_or_else(|| SchemaError::NotFound {
            kind: kind.clone(),
            api_version: api_version.to_owned(),
        })?;
        let result = compiled.validate(value);
        if let Err(errors) = result {
            let msgs: Vec<String> = errors.map(|e| e.to_string()).collect();
            return Err(SchemaError::Invalid { errors: msgs });
        }
        Ok(())
    }

    /// Return all registered `(kind, api_version)` pairs.
    pub fn list_kinds(&self) -> Vec<(&ResourceKind, &str)> {
        self.schemas
            .keys()
            .map(|(k, v)| (k, v.as_str()))
            .collect()
    }
}
```

---

**`src/shadow_store.rs`**

```rust
//! ShadowConfigStore — overlay for dry-run config validation.
//!
//! Phase 0: stub implementation only. The overlay type MUST exist so that
//! `sera-gateway` (Phase 2, SPEC-gateway §5.5) can reference `ShadowConfigStore`
//! without this being a compile-time blocker. The implementation will be filled in
//! during the self-evolution Phase 2 sprint.
//!
//! Design intent (SPEC-self-evolution §5.4, §7a):
//! A `ShadowConfigStore` wraps a prod `ConfigStore` and intercepts writes,
//! applying them to an in-memory overlay. Reads fall through to prod if the
//! overlay has no entry. This lets Change Artifacts validate config mutations
//! before committing them to the live store.

use crate::config_store::{ConfigStore, ConfigStoreError, ManifestValue};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory overlay over a prod `ConfigStore`.
///
/// Phase 0: only `overlay_put` and `get` (with fallthrough) are implemented.
/// `commit_overlay` and `diff` are stubbed and will be wired in Phase 2.
pub struct ShadowConfigStore<S: ConfigStore> {
    prod: Arc<S>,
    overlay: RwLock<HashMap<String, ManifestValue>>,
}

impl<S: ConfigStore> ShadowConfigStore<S> {
    pub fn new(prod: Arc<S>) -> Self {
        Self {
            prod,
            overlay: RwLock::new(HashMap::new()),
        }
    }

    /// Write a value into the shadow overlay (does NOT touch prod).
    pub fn overlay_put(&self, key: impl Into<String>, value: ManifestValue) {
        self.overlay.write().unwrap().insert(key.into(), value);
    }

    /// Read from the overlay; falls through to prod if the overlay has no entry.
    pub async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError> {
        {
            let guard = self.overlay.read().unwrap();
            if let Some(v) = guard.get(key) {
                return Ok(Some(v.clone()));
            }
        }
        self.prod.get(key).await
    }

    /// Returns true if the overlay has any pending mutations.
    pub fn is_dirty(&self) -> bool {
        !self.overlay.read().unwrap().is_empty()
    }

    /// Discard all overlay entries (rollback).
    pub fn discard(&self) {
        self.overlay.write().unwrap().clear();
    }

    /// Phase 2 stub — commit overlay mutations to the prod store.
    ///
    /// Not implemented in Phase 0. Callers that reach this in Phase 0 will panic
    /// with a clear message, making it obvious the Phase 2 work is missing.
    pub async fn commit_overlay(&self) -> Result<(), ConfigStoreError> {
        unimplemented!(
            "ShadowConfigStore::commit_overlay is a Phase 2 obligation — \
             see SPEC-self-evolution §5.4 and IMPL-AUDIT §2.2"
        )
    }
}
```

The `ConfigStore` trait and `ManifestValue` type referenced above live in the new `config_store.rs` module (see Files to modify section).

---

**`src/version_log.rs`**

```rust
//! ConfigVersionLog — append-only config change log with cryptographic hash chain.
//!
//! Required Phase 0 per SPEC-self-evolution §5.4 (IMPL-AUDIT §2.2).
//!
//! Each entry carries:
//! - `version`: monotonically increasing u64
//! - `change_artifact`: optional `ChangeArtifactId` for Tier 2/3 audit trail
//! - `signature`: Phase 2 obligation — `None` in Phase 0
//! - `prev_hash` / `this_hash`: SHA-256 chain
//!
//! The chain must never be truncated or rewritten. `append()` enforces this
//! by accepting only the next entry (verified against the tail hash).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Opaque identifier for a Change Artifact (Phase 2 design-forward).
/// Defined here as a newtype until `sera-domain` P0-1 lands the canonical type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChangeArtifactId(pub String);

/// A single entry in the version log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersionEntry {
    /// Monotonically increasing version counter starting at 1.
    pub version: u64,
    /// The Change Artifact that triggered this config change, if applicable.
    pub change_artifact: Option<ChangeArtifactId>,
    /// Detached signature over `this_hash` (Phase 2 obligation; `None` in Phase 0).
    pub signature: Option<Vec<u8>>,
    /// SHA-256 of the previous entry's `this_hash`; `[0u8; 32]` for the genesis entry.
    pub prev_hash: [u8; 32],
    /// SHA-256 of `(version_le_bytes ++ prev_hash ++ change_artifact_bytes ++ payload)`.
    pub this_hash: [u8; 32],
    /// Serialised config snapshot at this version.
    pub payload: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum VersionLogError {
    #[error("hash chain broken: entry {version} expected prev_hash {expected}, got {got}")]
    ChainBroken {
        version: u64,
        expected: String,
        got: String,
    },
    #[error("version log is empty")]
    Empty,
}

/// Append-only config version log with cryptographic hash chain.
///
/// Phase 0 implementation is in-memory. Phase 1 will back this with
/// a `sqlx` repository (SPEC-self-evolution §5.4 requires durability).
#[derive(Debug, Default)]
pub struct ConfigVersionLog {
    entries: Vec<ConfigVersionEntry>,
}

impl ConfigVersionLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tail hash of the last entry, or `[0u8; 32]` if the log is empty (genesis prev_hash).
    pub fn tail_hash(&self) -> [u8; 32] {
        self.entries.last().map(|e| e.this_hash).unwrap_or([0u8; 32])
    }

    /// Current version number (0 if empty).
    pub fn version(&self) -> u64 {
        self.entries.last().map(|e| e.version).unwrap_or(0)
    }

    /// Append a new config snapshot.
    ///
    /// `change_artifact` may be `None` for manual operator changes.
    /// Computes `this_hash` and returns the complete entry.
    pub fn append(
        &mut self,
        payload: serde_json::Value,
        change_artifact: Option<ChangeArtifactId>,
    ) -> ConfigVersionEntry {
        let prev_hash = self.tail_hash();
        let version = self.version() + 1;

        let mut hasher = Sha256::new();
        hasher.update(version.to_le_bytes());
        hasher.update(prev_hash);
        if let Some(ref ca) = change_artifact {
            hasher.update(ca.0.as_bytes());
        }
        hasher.update(payload.to_string().as_bytes());
        let hash_bytes: [u8; 32] = hasher.finalize().into();

        let entry = ConfigVersionEntry {
            version,
            change_artifact,
            signature: None,
            prev_hash,
            this_hash: hash_bytes,
            payload,
        };
        self.entries.push(entry.clone());
        entry
    }

    /// Verify the full hash chain from genesis to tail.
    /// Returns `Ok(n)` where `n` is the number of entries verified.
    pub fn verify_chain(&self) -> Result<usize, VersionLogError> {
        let mut expected_prev = [0u8; 32];
        for entry in &self.entries {
            if entry.prev_hash != expected_prev {
                return Err(VersionLogError::ChainBroken {
                    version: entry.version,
                    expected: hex_encode(expected_prev),
                    got: hex_encode(entry.prev_hash),
                });
            }
            expected_prev = entry.this_hash;
        }
        Ok(self.entries.len())
    }

    /// Read-only access to all entries (for inspection/export).
    pub fn entries(&self) -> &[ConfigVersionEntry] {
        &self.entries
    }
}

fn hex_encode(bytes: [u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
```

---

**`src/layer_merge.rs`**

```rust
//! ManifestSet layer-ordered merge (SPEC-config §3.3).
//!
//! Current `ManifestSet` is a flat accumulator with no merge precedence.
//! The target shape is layer-ordered: later layers (higher index) win on
//! field conflicts. Layer order: defaults → files → env → runtime overrides.
//!
//! Phase 0: provides the `LayeredManifestSet` type and `merge_layers()` function.
//! The existing `ManifestSet` is preserved for backwards compat; callers should
//! migrate to `LayeredManifestSet` in their respective P0 sprints.

use serde_json::{Map, Value};

/// A named layer in the manifest stack.
#[derive(Debug, Clone)]
pub struct ManifestLayer {
    /// Human-readable name for diagnostics (e.g. `"defaults"`, `"file:agents.yaml"`, `"env"`).
    pub name: String,
    /// The layer's manifest values as a JSON object.
    pub values: Map<String, Value>,
}

impl ManifestLayer {
    pub fn new(name: impl Into<String>, values: Map<String, Value>) -> Self {
        Self {
            name: name.into(),
            values,
        }
    }
}

/// Ordered stack of manifest layers.
///
/// Layer 0 = lowest precedence (defaults); last layer = highest precedence (runtime).
/// Matches `figment`'s merge semantics (IMPL-AUDIT §2.2 cites `figment = "0.10"`).
#[derive(Debug, Default)]
pub struct LayeredManifestSet {
    layers: Vec<ManifestLayer>,
}

impl LayeredManifestSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a layer onto the stack. Later calls have higher precedence.
    pub fn push(&mut self, layer: ManifestLayer) {
        self.layers.push(layer);
    }

    /// Merge all layers into a single JSON object. Later layers win on key conflicts.
    /// Merge is a shallow key-level merge (not deep/recursive) per §3.3 semantics.
    /// Deep merge of nested objects is a Phase 1 enhancement.
    pub fn merge(&self) -> Map<String, Value> {
        let mut result = Map::new();
        for layer in &self.layers {
            for (k, v) in &layer.values {
                result.insert(k.clone(), v.clone());
            }
        }
        result
    }

    /// Return the ordered layer names (lowest → highest precedence).
    pub fn layer_names(&self) -> Vec<&str> {
        self.layers.iter().map(|l| l.name.as_str()).collect()
    }

    /// Number of layers in the stack.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}
```

---

**`src/env_override.rs`**

```rust
//! Environment-variable override pattern for config (SPEC-config §3.1).
//!
//! Override format: `SERA_{KIND}_{NAME}_{FIELD}` (all uppercase, underscores).
//!
//! Example:
//!   `SERA_AGENT_my_agent_llm_model=gpt-4o`
//!   overrides the `llm_model` field of the agent manifest named `my_agent`.
//!
//! `scan_env_overrides()` returns a `ManifestLayer` ready to be pushed as the
//! highest-precedence layer in a `LayeredManifestSet`.

use crate::layer_merge::{LayeredManifestSet, ManifestLayer};
use serde_json::{Map, Value};
use std::env;

const ENV_PREFIX: &str = "SERA_";

/// Scan the process environment for `SERA_*` overrides and return a layer.
///
/// Only variables matching `SERA_{KIND}_{NAME}_{FIELD}` with at least three
/// underscore-separated segments after the prefix are collected. Variables
/// with fewer segments are ignored (e.g. `SERA_CORE_URL` is a top-level
/// env var handled by `SeraConfig::from_env()`, not a manifest override).
pub fn scan_env_overrides() -> ManifestLayer {
    let mut values = Map::new();

    for (key, val) in env::vars() {
        if !key.starts_with(ENV_PREFIX) {
            continue;
        }
        let rest = &key[ENV_PREFIX.len()..];
        let parts: Vec<&str> = rest.splitn(3, '_').collect();
        if parts.len() < 3 {
            continue; // not a manifest override
        }
        // Reconstruct as a dotted path `kind.name.field` for clarity in diagnostics.
        let dotted = parts.join(".");
        values.insert(dotted, Value::String(val));
    }

    ManifestLayer::new("env", values)
}

/// Convenience: build a `LayeredManifestSet` from an existing set of layers
/// and apply env overrides as the top (highest precedence) layer.
pub fn apply_env_overrides(base: &mut LayeredManifestSet) {
    let env_layer = scan_env_overrides();
    if !env_layer.values.is_empty() {
        base.push(env_layer);
    }
}
```

---

**`src/config_store.rs`** (new — required by `shadow_store.rs`)

```rust
//! ConfigStore trait and ManifestValue type (SPEC-config §7a).
//!
//! `ConfigStore` is the minimal interface both `LiveConfigStore` and
//! `ShadowConfigStore` implement. Phase 0 provides the trait and a stub
//! `ManifestValue` type. `LiveConfigStore` will be implemented in the
//! P0-3 execution sprint when sqlx/figment integration lands.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A serialised manifest value. Phase 0 representation is a raw JSON value.
/// Phase 1 will carry richer typed enums per resource kind.
pub type ManifestValue = serde_json::Value;

#[derive(Debug, Error)]
pub enum ConfigStoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("serialisation error: {0}")]
    Serialise(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// Minimal async config store interface (SPEC-config §7a).
#[async_trait]
pub trait ConfigStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError>;
    async fn list(&self, prefix: &str) -> Result<Vec<(String, ManifestValue)>, ConfigStoreError>;
    async fn version(&self) -> Result<u64, ConfigStoreError>;
}
```

---

### Files to modify

**`src/lib.rs`** — add new module declarations and re-exports:

```rust
// Add to existing module list:
pub mod config_store;
pub mod env_override;
pub mod layer_merge;
pub mod schema_registry;
pub mod shadow_store;
pub mod version_log;

// Add re-exports:
pub use config_store::{ConfigStore, ConfigStoreError, ManifestValue};
pub use layer_merge::{LayeredManifestSet, ManifestLayer};
pub use schema_registry::SchemaRegistry;
pub use shadow_store::ShadowConfigStore;
pub use version_log::{ChangeArtifactId, ConfigVersionEntry, ConfigVersionLog};
```

**`Cargo.toml`** — add new dependencies:

```toml
[dependencies]
# existing deps unchanged ...
figment = { workspace = true }
schemars = { workspace = true }
jsonschema = { workspace = true }
sha2 = "0.10"
async-trait.workspace = true
once_cell = "1"

[features]
default = []
# Enables jsonschema validation at manifest load time (off by default in
# sera-runtime container builds to reduce binary size).
schema-validation = []
```

The `schema-validation` feature gates `jsonschema` calls inside `SchemaRegistry::validate()` — when the feature is absent, `validate()` is a no-op returning `Ok(())`. This allows `sera-runtime` container builds to skip the `jsonschema` compile dependency. The `SchemaRegistry` type itself is always present (gating the type itself would break `sera-gateway` which always wants it).

**`src/manifest_loader.rs`** — add `ManifestSet` layer-ordering note and a `TryFrom<LayeredManifestSet>` path. The existing `ManifestSet` struct is preserved unchanged; the new `LayeredManifestSet` sits alongside it. Callers can opt in to the new merge semantics without a flag day.

---

### Workspace dependency additions

Add to `[workspace.dependencies]` in `rust/Cargo.toml`:

```toml
figment    = "0.10"     # layered config resolution: defaults → files → env → runtime
schemars   = "0.8"      # JSON Schema generation from Rust types via schema_for!()
jsonschema = "0.38"     # compile-time validated JSON Schema checks at manifest load
```

No exact-equals pins required here (these are not ABI-coupled the way the OTel triad is).

---

### Cargo features

| Feature | Default | Purpose |
|---|---|---|
| `default` | yes (empty) | Core types, `LayeredManifestSet`, `ConfigVersionLog`, `ShadowConfigStore` stub |
| `schema-validation` | opt-in | Activates `jsonschema` validation calls in `SchemaRegistry::validate()`; adds `jsonschema` compile dep. Disabled for `sera-runtime` container builds. |

---

### Acceptance tests

Place in `rust/crates/sera-config/tests/`.

**`tests/test_shadow_store.rs`**

```rust
//! ShadowConfigStore overlay shadows prod values without mutating prod.

use sera_config::shadow_store::ShadowConfigStore;
use sera_config::config_store::{ConfigStore, ConfigStoreError, ManifestValue};
use async_trait::async_trait;
use std::sync::Arc;
use serde_json::json;

struct ProdStore;

#[async_trait]
impl ConfigStore for ProdStore {
    async fn get(&self, key: &str) -> Result<Option<ManifestValue>, ConfigStoreError> {
        match key {
            "model" => Ok(Some(json!("gpt-4o-mini"))),
            _ => Ok(None),
        }
    }
    async fn list(&self, _: &str) -> Result<Vec<(String, ManifestValue)>, ConfigStoreError> {
        Ok(vec![("model".into(), json!("gpt-4o-mini"))])
    }
    async fn version(&self) -> Result<u64, ConfigStoreError> { Ok(1) }
}

#[tokio::test]
async fn shadow_overlay_shadows_prod_value() {
    let store = ShadowConfigStore::new(Arc::new(ProdStore));
    // Prod value is "gpt-4o-mini"
    let prod_val = store.get("model").await.unwrap();
    assert_eq!(prod_val, Some(json!("gpt-4o-mini")));

    // Shadow overlay replaces it
    store.overlay_put("model", json!("claude-opus-4"));
    let shadow_val = store.get("model").await.unwrap();
    assert_eq!(shadow_val, Some(json!("claude-opus-4")));
}

#[tokio::test]
async fn shadow_fallthrough_returns_prod_for_unset_keys() {
    let store = ShadowConfigStore::new(Arc::new(ProdStore));
    let val = store.get("model").await.unwrap();
    assert_eq!(val, Some(json!("gpt-4o-mini")));
}

#[tokio::test]
async fn shadow_discard_removes_overlay() {
    let store = ShadowConfigStore::new(Arc::new(ProdStore));
    store.overlay_put("model", json!("o3"));
    assert!(store.is_dirty());
    store.discard();
    assert!(!store.is_dirty());
    // Falls through to prod after discard
    let val = store.get("model").await.unwrap();
    assert_eq!(val, Some(json!("gpt-4o-mini")));
}
```

**`tests/test_version_log.rs`**

```rust
//! ConfigVersionLog hash chain validation.

use sera_config::version_log::{ChangeArtifactId, ConfigVersionLog};
use serde_json::json;

#[test]
fn version_log_starts_empty() {
    let log = ConfigVersionLog::new();
    assert_eq!(log.version(), 0);
    assert_eq!(log.tail_hash(), [0u8; 32]);
}

#[test]
fn version_log_append_increments_version() {
    let mut log = ConfigVersionLog::new();
    let e1 = log.append(json!({"model": "gpt-4o"}), None);
    assert_eq!(e1.version, 1);
    let e2 = log.append(json!({"model": "claude-opus-4"}), None);
    assert_eq!(e2.version, 2);
}

#[test]
fn version_log_chain_verifies_after_appends() {
    let mut log = ConfigVersionLog::new();
    log.append(json!({"a": 1}), None);
    log.append(json!({"a": 2}), Some(ChangeArtifactId("ca-001".into())));
    log.append(json!({"a": 3}), None);
    let count = log.verify_chain().unwrap();
    assert_eq!(count, 3);
}

#[test]
fn version_log_prev_hash_chain_is_linked() {
    let mut log = ConfigVersionLog::new();
    let e1 = log.append(json!({}), None);
    let e2 = log.append(json!({}), None);
    // e2's prev_hash must equal e1's this_hash
    assert_eq!(e2.prev_hash, e1.this_hash);
}

#[test]
fn version_log_genesis_prev_hash_is_zero() {
    let mut log = ConfigVersionLog::new();
    let entry = log.append(json!({"init": true}), None);
    assert_eq!(entry.prev_hash, [0u8; 32]);
}
```

**`tests/test_schema_registry.rs`**

```rust
//! SchemaRegistry round-trip and validation.

use sera_config::schema_registry::SchemaRegistry;
// Note: this test is illustrative — ResourceKind requires the P0-1 sera-domain
// sprint to land the 13-variant enum. Adjust the import path accordingly once
// sera-domain is renamed to sera-types.
use serde_json::json;

// Minimal stub for test until ResourceKind is extended in P0-1
// (uses schemars for the round-trip assertion without a real ResourceKind dep).
#[test]
fn schema_registry_validate_rejects_invalid_payload() {
    // Uses serde_json directly to simulate schema validation logic.
    // Full integration test with real ResourceKind belongs in P0-1 follow-up.
    let schema_json = json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "required": ["name"]
    });
    let compiled = jsonschema::JSONSchema::compile(&schema_json).unwrap();

    // Valid
    assert!(compiled.validate(&json!({"name": "agent-1"})).is_ok());
    // Invalid — missing required field
    assert!(compiled.validate(&json!({})).is_err());
}
```

**`tests/test_env_override.rs`**

```rust
//! Env-var override precedence: env layer wins over base layers.

use sera_config::layer_merge::{LayeredManifestSet, ManifestLayer};
use serde_json::{json, Map, Value};

#[test]
fn env_override_higher_precedence_than_base() {
    let mut base_values = Map::new();
    base_values.insert("AGENT.my_agent.model".into(), json!("gpt-4o-mini"));

    let mut stack = LayeredManifestSet::new();
    stack.push(ManifestLayer::new("defaults", base_values));

    // Simulate env override layer (higher precedence)
    let mut env_values = Map::new();
    env_values.insert("AGENT.my_agent.model".into(), json!("claude-opus-4"));
    stack.push(ManifestLayer::new("env", env_values));

    let merged = stack.merge();
    assert_eq!(merged["AGENT.my_agent.model"], json!("claude-opus-4"));
}

#[test]
fn layer_merge_base_value_survives_when_no_env_override() {
    let mut base = Map::new();
    base.insert("AGENT.my_agent.timeout".into(), json!(30));
    let mut stack = LayeredManifestSet::new();
    stack.push(ManifestLayer::new("file", base));
    let merged = stack.merge();
    assert_eq!(merged["AGENT.my_agent.timeout"], json!(30));
}
```

**`tests/test_layer_merge.rs`**

```rust
//! figment layer merge order: last layer wins.

use sera_config::layer_merge::{LayeredManifestSet, ManifestLayer};
use serde_json::{json, Map};

#[test]
fn later_layer_wins_on_conflict() {
    let mut stack = LayeredManifestSet::new();

    let mut l1 = Map::new();
    l1.insert("key".into(), json!("layer1"));
    stack.push(ManifestLayer::new("layer1", l1));

    let mut l2 = Map::new();
    l2.insert("key".into(), json!("layer2"));
    stack.push(ManifestLayer::new("layer2", l2));

    let merged = stack.merge();
    assert_eq!(merged["key"], json!("layer2"), "layer2 must win over layer1");
}

#[test]
fn non_overlapping_keys_are_all_present() {
    let mut stack = LayeredManifestSet::new();
    let mut l1 = Map::new();
    l1.insert("a".into(), json!(1));
    stack.push(ManifestLayer::new("defaults", l1));
    let mut l2 = Map::new();
    l2.insert("b".into(), json!(2));
    stack.push(ManifestLayer::new("env", l2));

    let merged = stack.merge();
    assert_eq!(merged["a"], json!(1));
    assert_eq!(merged["b"], json!(2));
}

#[test]
fn layer_names_in_push_order() {
    let mut stack = LayeredManifestSet::new();
    stack.push(ManifestLayer::new("defaults", Map::new()));
    stack.push(ManifestLayer::new("file", Map::new()));
    stack.push(ManifestLayer::new("env", Map::new()));
    assert_eq!(stack.layer_names(), vec!["defaults", "file", "env"]);
}
```

---

### Acceptance tests summary

| File | Function | Assertion |
|---|---|---|
| `tests/test_shadow_store.rs` | `shadow_overlay_shadows_prod_value` | Overlay value takes precedence over prod value |
| `tests/test_shadow_store.rs` | `shadow_fallthrough_returns_prod_for_unset_keys` | Unset overlay key falls through to prod |
| `tests/test_shadow_store.rs` | `shadow_discard_removes_overlay` | `discard()` clears overlay and restores prod fallthrough |
| `tests/test_version_log.rs` | `version_log_starts_empty` | Empty log has version 0 and zero tail hash |
| `tests/test_version_log.rs` | `version_log_append_increments_version` | Each append increments version by 1 |
| `tests/test_version_log.rs` | `version_log_chain_verifies_after_appends` | `verify_chain()` returns correct entry count |
| `tests/test_version_log.rs` | `version_log_prev_hash_chain_is_linked` | `entry[n].prev_hash == entry[n-1].this_hash` |
| `tests/test_version_log.rs` | `version_log_genesis_prev_hash_is_zero` | Genesis entry has `prev_hash = [0u8; 32]` |
| `tests/test_schema_registry.rs` | `schema_registry_validate_rejects_invalid_payload` | Invalid JSON is rejected, valid JSON passes |
| `tests/test_env_override.rs` | `env_override_higher_precedence_than_base` | Env layer wins over base layer on same key |
| `tests/test_env_override.rs` | `layer_merge_base_value_survives_when_no_env_override` | Unoverridden base value is preserved |
| `tests/test_layer_merge.rs` | `later_layer_wins_on_conflict` | Last-pushed layer wins |
| `tests/test_layer_merge.rs` | `non_overlapping_keys_are_all_present` | All non-conflicting keys from all layers present |
| `tests/test_layer_merge.rs` | `layer_names_in_push_order` | `layer_names()` reflects push order |

---

### Downstream cascade

`sera-config` is currently consumed by `sera-core` and `sera-runtime`. Version bumps from the new modules ripple as follows:

| Crate | Impact |
|---|---|
| `sera-gateway` (renamed from `sera-core`) | Gains `SchemaRegistry` at boot (register schemas for all 13 `ResourceKind` variants), `ShadowConfigStore` for dry-run mutation path (Phase 2), `ConfigVersionLog` for the change-artifact audit trail |
| `sera-runtime` | Gains `SeraConfig` (unchanged), `LayeredManifestSet` for manifest resolution inside the agent container. The `schema-validation` feature is OFF by default for runtime builds to avoid pulling `jsonschema` into the container image |
| `sera-hooks` | No direct dep on `sera-config` today. Phase 1 constitutional gate will need `SchemaRegistry` to validate `ConstitutionalRule` payloads — this will be wired in the P1 hooks sprint |
| `sera-db` | No new dep. `ConfigVersionLog` Phase 1 persistence will be backed by a new `config_version_log` table in the existing postgres schema — the sqlx repository will live in `sera-db` and call `sera-config::ConfigVersionLog::append()` |
| `sera-auth` | No new dep at Phase 0. Phase 2 will want `ShadowConfigStore` for policy dry-runs |

The `sha2 = "0.10"` dep added to `sera-config` is already present in the workspace via `sera-events` (same version). Once `sera-events` is deleted, ensure `sha2` is retained in the workspace `[dependencies]` or promoted to `[workspace.dependencies]` to avoid version drift.
## P0-4 · sera-db / sera-queue split

### Strategy

`sera-db` currently embeds `lane_queue.rs` (~597 LoC, five queue modes, global concurrency cap) as a module. SPEC-crate-decomposition §3 and SPEC-dependencies §8.3 mandate a standalone `sera-queue` crate built as a thin trait over `apalis` 0.7. The extraction is a Phase 0 blocker because the runtime turn loop (P0-6) depends on `apalis` being wired before its doom-loop and steer-at-tool-boundary logic can be assembled.

**Extraction approach:**

1. Create `rust/crates/sera-queue/` as a new `[lib]` workspace member.
2. Move `lane_queue.rs` contents wholesale into `sera-queue/src/lane.rs` (preserves 597 LoC + test suite). Orphan-recovery gap (§4.7) addressed in `src/local.rs` via sqlx write-through.
3. Define `QueueBackend` trait in `src/backend.rs`; `LocalQueueBackend` wraps `LaneQueue`; `ApalisBackend` wraps `apalis 0.7`.
4. `sera-db` drops queue module, adds `migrations/` skeleton + `MigrationKind` enum.
5. `sera-core/src/bin/sera.rs` has 8 import sites at `sera_db::lane_queue::` — update to `sera_queue::`.

### sera-queue files to create

All paths under `rust/crates/sera-queue/`:

- **`Cargo.toml`** — name, features `default = ["local"]`, `apalis = ["dep:apalis", "dep:apalis-sql"]`. Deps: sera-types, serde, tokio, tracing, thiserror, uuid, async-trait.
- **`src/lib.rs`** — pub mods backend/lane/throttle/cron/local/apalis with feature gates; re-exports.
- **`src/backend.rs`** — `QueueBackend` async trait: `push`, `pull`, `ack`, `nack`, `recover_orphans(stale_threshold)`. Object-safe, `Send + Sync + 'static`. Associated type `Job: Serialize + DeserializeOwned`. `QueueError` enum (Unavailable, Serde, NotFound, Storage).
- **`src/local.rs`** — `LocalQueueBackend` wraps `LaneQueue` behind `tokio::sync::Mutex`. Optional sqlx `PersistenceStore` for write-through orphan log (`queue_orphan_log` table). `recover_orphans()` reads stale rows, re-enqueues.
- **`src/lane.rs`** — `LaneQueue`, `QueueMode { Collect, Followup, Steer, SteerBacklog, Interrupt }`, `QueuedEvent`, `EnqueueResult`. Direct move from sera-db.
- **`src/apalis.rs`** — `#[cfg(feature = "apalis")]` `ApalisBackend` wrapping `apalis::prelude::WorkerBuilder` + `apalis_sql::PostgresStorage`.
- **`src/throttle.rs`** — `GlobalThrottle { cap, semaphore: Arc<Semaphore> }` extracted from LaneQueue counters.
- **`src/cron.rs`** — `CronScheduler::schedule(expr, factory) -> JoinHandle`. Thin wrapper over apalis cron (feature-gated) or tokio fallback.

### sera-db files to modify

- **`src/lib.rs`** — remove `pub mod lane_queue;`. Add `pub mod migration_kind;`.
- **`src/migration_kind.rs`** (new) — `#[non_exhaustive] enum MigrationKind { Reversible, ForwardOnlyWithPairedOut, Irreversible }`. `requires_down_file(self)` helper.
- **`migrations/`** (new directory):
  - `0001_initial_schema.up.sql` / `.down.sql`
  - `0002_queue_orphan_log.up.sql` / `.down.sql`

### Workspace dep additions

```toml
apalis        = "0.7"
apalis-sql    = "0.7"
async-trait   = "0.1"
sera-queue    = { path = "crates/sera-queue" }
```

Add `crates/sera-queue` to `[workspace] members`.

### Cargo features

- **sera-queue**: `default = ["local"]`, `local = []`, `apalis = ["dep:apalis", "dep:apalis-sql"]`. Additive.
- **sera-db**: No new feature flags. `sea-orm` prohibited by absence.

### Acceptance tests

| # | Test | Location | Verifies |
|---|------|----------|----------|
| 1 | `orphan_recovery_across_restart` | `local.rs` | Simulated restart re-enqueues unack'd jobs |
| 2 | `migration_kind_requires_down_file_reversible` | `migration_kind.rs` | Reversible requires down file |
| 3 | `migration_kind_requires_down_file_irreversible` | `migration_kind.rs` | Irreversible does not |
| 4 | `migration_kind_exhaustiveness_compile_check` | `migration_kind.rs` | `#[non_exhaustive]` forces `_` arm |
| 5 | `apalis_cron_fires` | `cron.rs` (feat-gated) | Job fires within 2× tick interval |
| 6 | `local_queue_fifo_per_lane` | `lane.rs` | Followup mode preserves FIFO |
| 7 | `global_throttle_cap_blocks_dequeue` | `lane.rs` | Cap=2, active=2 → None on third |
| 8 | `global_throttle_releases_on_complete_run` | `lane.rs` | complete_run decrements |
| 9 | `reversibility_contract_compile_check` | `migration_kind.rs` | `#[non_exhaustive]` downstream match |
| 10 | `local_backend_push_pull_ack_roundtrip` | `local.rs` | Queue depth returns to 0 |
| 11 | `steer_newest_wins` | `lane.rs` | take_steer returns latest |
| 12 | `interrupt_clears_backlog` | `lane.rs` | Interrupt resets lane depth |

Tests 6-8, 11-12 migrate verbatim from existing lane_queue tests.

### Downstream cascade

**`sera-core/src/bin/sera.rs`** 9 import sites (lines 33, 120, 1477, 1554, 1568, 2095, 2112, 2130, 2148) change `sera_db::lane_queue::` → `sera_queue::`. After P0-5, the new `sera-gateway/Cargo.toml` must list `sera-queue.workspace = true`.

`LocalQueueBackend` uses sqlx directly (not via sera-db) with `sqlx::PgPool` parameter, avoiding circular dep.

---

## P0-8 · sera-docker → sera-tools absorption

### Strategy

Stand up new `rust/crates/sera-tools/`. Move `ContainerManager` + `DockerEventListener` into `sera-tools/src/sandbox/docker.rs` as `DockerSandboxProvider` impl. **Retain `sera-docker` as a non-published compatibility shim** (re-exports the moved types) until P0-5/P0-6 finalise the gateway/runtime rewrites — `sera-core/src/bin/sera.rs`, `src/main.rs`, `src/state.rs`, `src/services/cleanup.rs`, `src/services/orchestrator.rs` all import ContainerManager directly. Delete `sera-docker` after P0-5/P0-6. Do **not** create a peer `sera-sandbox` crate.

### sera-tools files to create

All paths under `rust/crates/sera-tools/`:

- **`Cargo.toml`** — `publish = true`. Features: `default = ["docker"]`, `docker = ["dep:bollard", "dep:futures-util"]`, `wasm = ["dep:wasmtime", "dep:wasmtime-wasi"]`, `microvm/external/openshell = []` (stubs). Deps: sera-types, serde, tokio, tracing, thiserror, uuid, sha2, hex, async-trait, regorus.
- **`src/lib.rs`** — mods registry, sandbox, ssrf, binary_identity, bash_ast, inference_local, kill_switch.
- **`src/registry.rs`** — `ToolRegistry { HashMap<String, Arc<dyn Tool>> }` with register/get/list.
- **`src/sandbox/mod.rs`** — `SandboxProvider` async trait (object-safe): `name`, `create(config)`, `execute(handle, command, env)`, `read_file`, `write_file`, `destroy`, `status`. `SandboxHandle` newtype, `SandboxConfig`, `ExecResult { exit_code, stdout, stderr }`.
- **`src/sandbox/policy.rs`** — three-layer:
  - `#[non_exhaustive] SandboxPolicy { Docker(DockerSandboxPolicy), Wasm(..), MicroVm(..), External(..), OpenShell(..), None }`
  - `FileSystemSandboxPolicy { read_paths, write_paths, include_workdir }`
  - `NetworkSandboxPolicy { rules: Vec<NetworkPolicyRule>, default_deny }`
  - `NetworkPolicyRule { endpoint, action, l7_rules }`; `NetworkEndpoint { Cidr, Domain, InferenceLocal }`; `PolicyAction { Allow, Deny, Audit }`; `L7Rule { protocol, path_prefix }`; `L7Protocol { Http, Https, Grpc }`.
  - `PolicyStatus { version, content_hash: [u8;32], loaded_at }`.
- **`src/sandbox/docker.rs`** — `#[cfg(feature="docker")] DockerSandboxProvider { inner: bollard::Docker, policy_status }`. Absorbs ContainerManager + DockerEventListener; Centrifugo calls replaced with `Box<dyn AuditHandle>` (stubbed `NoOpAuditHandle` until sera-telemetry lands). Labels preserved: sera.sandbox, sera.agent, sera.instance, sera.type, sera.template, sera.managed. `start_container` signature `(tier: Option<u32>)` → `(config: SandboxConfig)`.
- **`src/sandbox/wasm.rs`** — `WasmSandboxProvider` stub. All methods return `SandboxError::NotImplemented`.
- **`src/sandbox/microvm.rs`** — stub.
- **`src/sandbox/external.rs`** — delegates to external process; stub in Phase 0.
- **`src/sandbox/openshell.rs`** — Tier-3 stub per OpenShell proto vendoring (P1).
- **`src/ssrf.rs`** — `SsrfValidator::validate(addr, policy)` blocks loopback (127.0.0.0/8, ::1), link-local (169.254.0.0/16, fe80::/10), cloud metadata (169.254.169.254, 100.100.100.200), RFC-1918 unless allowed. `SsrfError { Loopback, LinkLocal, CloudMetadata, NotAllowed, ParseError }`.
- **`src/binary_identity.rs`** — `NetworkBinary { path, tofu_sha256: [u8;32] }`, `BinaryIdentity { store: RwLock<HashMap<PathBuf, [u8;32]>> }` with `verify_or_pin(path)` TOFU.
- **`src/bash_ast.rs`** — `BashAstChecker::check(command, policy)`. Phase 0: hand-rolled shell tokenizer (tree-sitter deferred to P1). Blocks backtick substitution, process substitution, out-of-sandbox path access, shell metachar injection.
- **`src/inference_local.rs`** — `InferenceLocalResolver::rewrite(url)` rewrites `inference.local` virtual host to gateway endpoint.
- **`src/kill_switch.rs`** — `KillSwitch` binds Unix socket `/var/lib/sera/admin.sock`. OS-level file-ownership auth (bypasses normal stack). Command: `ROLLBACK`. Returns `(Self, broadcast::Receiver<KillSwitchCommand>)`. `boot_health_check()` is CON-04 compliance point — gateway refuses startup on Err.

### sera-docker migration — symbols to move

| Symbol | Source | Destination |
|--------|--------|-------------|
| `ContainerManager` + impl | `sera-docker/src/container.rs` | `sera-tools/src/sandbox/docker.rs` as `DockerSandboxProvider` |
| `ExecOutput` | `sera-docker/src/lib.rs` | `sera-tools/src/sandbox/mod.rs` as `ExecResult` |
| `DockerEventListener` | `sera-docker/src/events.rs` | same file as provider |
| `DockerEvent` | `sera-docker/src/events.rs` | same |
| `DockerError` | `sera-docker/src/error.rs` | as `DockerSandboxError`, unified under `SandboxError` |

After migration, `sera-docker/src/lib.rs` becomes re-export shim. `sera-docker/Cargo.toml` gets `publish = false` and `sera-tools.workspace = true`.

### Workspace dep additions

```toml
sera-tools    = { path = "crates/sera-tools" }
regorus       = "0.3"
wasmtime      = ">=43, <50"
wasmtime-wasi = ">=43, <50"
```

`bollard 0.18` already present.

### Cargo features

```toml
default = ["docker"]
docker = ["dep:bollard", "dep:futures-util"]
wasm = ["dep:wasmtime", "dep:wasmtime-wasi"]
microvm = []        # stub
external = []       # stub
openshell = []      # stub
```

### Acceptance tests

| # | Test | Location | Verifies |
|---|------|----------|----------|
| 1 | `sandbox_provider_trait_is_object_safe` | `sandbox/mod.rs` | `Box<dyn SandboxProvider>` compiles |
| 2 | `docker_provider_implements_trait` | `sandbox/docker.rs` | Trait impl check via `fn assert_impl<T: SandboxProvider>()` |
| 3 | `policy_layering_coarse_plus_fs_plus_network` | `sandbox/policy.rs` | 3-layer JSON roundtrip |
| 4 | `policy_status_hash_changes_on_modification` | `sandbox/policy.rs` | Content hash stability |
| 5 | `ssrf_validator_blocks_loopback` | `ssrf.rs` | 127.0.0.1 → Loopback err |
| 6 | `ssrf_validator_blocks_link_local` | `ssrf.rs` | 169.254.1.1 → LinkLocal |
| 7 | `ssrf_validator_blocks_metadata_endpoint` | `ssrf.rs` | 169.254.169.254 → CloudMetadata |
| 8 | `ssrf_validator_allows_explicit_cidr` | `ssrf.rs` | Policy Allow 10.0.0.0/8 permits 10.1.2.3 |
| 9 | `tofu_binary_identity_pins_on_first_use` | `binary_identity.rs` | First-use OK, tamper → HashMismatch |
| 10 | `tofu_binary_identity_accepts_unchanged_binary` | `binary_identity.rs` | Stable path returns OK |
| 11 | `kill_switch_boot_health_check_fails_closed` | `kill_switch.rs` | Bad path → Err, no panic |
| 12 | `kill_switch_bind_and_receive_rollback` | `kill_switch.rs` | Write `ROLLBACK\n` → receive command |
| 13 | `con04_boot_check_passes_with_valid_socket` | `kill_switch.rs` | Tempdir → Ok |
| 14 | `inference_local_rewrites_url` | `inference_local.rs` | `inference.local/v1/chat` → configured endpoint |
| 15 | `bash_ast_checker_blocks_backtick_substitution` | `bash_ast.rs` | `` ls `id` `` → Denied |

### Downstream cascade (post-shim removal)

Call sites requiring migration after P0-5/P0-6:

| File:line | Current | Target |
|-----------|---------|--------|
| `sera-core/src/main.rs:31` | `sera_docker::ContainerManager` | `sera_tools::sandbox::docker::DockerSandboxProvider` |
| `sera-core/src/state.rs:10` | same | same |
| `sera-core/src/services/cleanup.rs:5,30` | `sera_docker::ContainerManager`, `DockerError` | `DockerSandboxProvider`, `DockerSandboxError` |
| `sera-core/src/services/orchestrator.rs:11,289` | `sera_docker::container::ContainerManager`, `DockerError` | same |

In Phase 0, shim re-exports keep these compiling unchanged.

---

## P0-10 (partial) · infrastructure scaffolding

| Crate | Layer | Relation |
|-------|-------|----------|
| `sera-errors` | Foundation | Required by sera-queue and sera-tools. `QueueError`/`SandboxError` should wrap a shared `SeraErrorCode`. Scaffold in same PR. No deps on P0-4/P0-8. |
| `sera-cache` | Infrastructure | Independent. Scaffold `CacheBackend` trait (`moka` + `fred` stubs) in parallel. |
| `sera-secrets` | Infrastructure | DockerSandboxProvider needs secret injection at create(). `Box<dyn SecretsProvider>` field. Scaffold with `EnvSecretsProvider` (SERA_SECRET_* env vars) same milestone as P0-8. |
| `sera-testing` | Dev-only | `publish = false`. Mock `SandboxProvider` (unit-test sera-tools without Docker), mock `QueueBackend`. Depends on both. Scaffold immediately after P0-4/P0-8. |
| `sera-telemetry` | Infrastructure | `AuditHandle` trait + `NoOpAuditHandle` stub. sera-tools uses locally-defined stub in Phase 0, swaps to `sera_telemetry::AuditHandle` when that crate lands. Avoids circular dep. |
| `sera-session` | Core Domain | Depends on sera-queue + sera-tools. Scaffold 6-state machine + ContentBlock transcript after P0-4/P0-8. Cannot start before QueueBackend trait is stable. |

**Ordering:**

```
sera-errors  ──┐
               ├──► sera-queue  ──┐
sera-secrets ──┘                  ├──► sera-session
                                  │
               ┌──► sera-tools  ──┘
sera-cache   ──┘
                    sera-testing (after both, dev-only)
```
## P0-5 · sera-core → sera-gateway

### Dependency note

Depends on **P0-1 (sera-types)** landing first. `Submission`, `EventContext`, `GenerationMarker`, `ChangeArtifactId`, `BuildIdentity` are defined in sera-types. Do not begin SQ/EQ type additions until the sera-types rename commit is merged.

### Rename strategy

```
git mv rust/crates/sera-core rust/crates/sera-gateway
```

Update references:
1. `rust/Cargo.toml` members: `"crates/sera-core"` → `"crates/sera-gateway"`
2. `rust/crates/sera-gateway/Cargo.toml`: `name = "sera-gateway"`; both `[[bin]]` entries (`sera-gateway` and `sera`)
3. `rust/CLAUDE.md`: crate map row

**No sibling crate declares sera-core as a dependency** — sera-core is a binary at the top of the graph. Rename touches exactly 3 files.

Rename lands in a separate commit from SQ/EQ structural changes (clean diff history).

### Files to create under `rust/crates/sera-gateway/src/`

- **`src/envelope.rs`** — SQ/EQ per SPEC-gateway §3.1–§3.2:
  - `Submission { id: Uuid, op: Op, trace: W3cTraceContext, change_artifact: Option<ChangeArtifactId> }`
  - `Op` enum: `UserTurn { items: Vec<ContentBlock>, cwd, approval_policy, sandbox_policy, model_override, effort, final_output_schema } | Steer { items } | Interrupt | System(SystemOp) | ApprovalResponse { approval_id, decision } | Register(RegisterOp)`
  - `Event { id, submission_id, msg: EventMsg, trace, timestamp }`
  - `EventMsg` covering streaming delta, turn lifecycle, HITL request, compaction, session transition, error
  - `EventContext { agent_id, session_key, sender, recipient, principal, cause_by: Option<ActionId>, parent_session_key, generation: GenerationMarker, metadata }`
  - `DedupeKey { channel, account, peer, session_key, message_id }`
  - `QueueMode { Collect, Followup, Steer, SteerBacklog, Interrupt }`
  - `WorkerFailureKind { TrustGate, PromptDelivery, Protocol, Provider }`

- **`src/transport/mod.rs`** — `AppServerTransport` spine per SPEC-gateway §7a:
  ```
  AppServerTransport {
      InProcess,
      Stdio { command, args, env },
      WebSocket { bind, tls },
      Grpc { endpoint, tls },
      WebhookBack { callback_base_url, session_api_key_generator },
      Off,
  }
  ```
  Every harness MUST provide `InProcess` (compile-time contract). `Transport` async trait: `send_submission(s)`, `recv_events() -> impl Stream<Item = Event>`. `TransportConfig` serde from manifest.

- **`src/transport/in_process.rs`** — `InProcessTransport` with mpsc channel pair. Default, always compiled.
- **`src/transport/stdio.rs`** — spawns child via `tokio::process::Command`; NDJSON on stdin/stdout. `#[cfg(feature = "stdio")]`.
- **`src/transport/websocket.rs`** — tokio-tungstenite listener. Optional TLS via tokio-rustls. `#[cfg(feature = "websocket")]`.
- **`src/transport/grpc.rs`** — tonic bidirectional streaming. Proto at `rust/proto/sera_gateway.proto`. `#[cfg(feature = "grpc")]`.
- **`src/transport/webhook_back.rs`** — serverless-harness pattern. Per-session API key. `#[cfg(feature = "webhook-back")]`.

- **`src/harness_dispatch.rs`** — central dispatch layer:
  - `HarnessRegistry = Arc<RwLock<HashMap<AgentId, Box<dyn AgentHarness>>>>`
  - `AgentHarness` trait (coordinate with Agent A — likely lives in sera-types to avoid cycles)
  - `dispatch(submission, &registry, &transport) -> impl Stream<Item = Event>`
  - `PluginRegistry` — separate registry for plugin hooks

- **`src/session_persist.rs`** — two-layer per SPEC-gateway §6.1b:
  - `PartTable` — sqlx stream to `session_parts` table per tool call / text block / reasoning step
  - `SessionSnapshot` — shadow git at `~/.local/share/sera/snapshot/<session_key>/`; `track`, `revert`, `diff_full`
  - Requires `git2` workspace dep

- **`src/kill_switch.rs`** — admin socket at `/var/lib/sera/admin.sock`, OS file-ownership auth (bypasses auth stack intentionally). Command `ROLLBACK\n` → cancel all turns, set `KillSwitchState::Armed`. CON-04 boot-time check before router starts. Coordinate with sera-tools kill switch.

- **`src/generation.rs`** — `GenerationMarker { label, binary_identity, started_at }`. Constructed at startup from `env!("CARGO_PKG_VERSION")` + build-time git SHA. Copied into every EventContext. Phase 0 stub (not yet acted upon by policy).

- **`src/connector/mod.rs`** — `ConnectorRegistry`, `Connector` trait (`deliver`, `receive`). Discord bridge reference impl; feature-gated `connector-discord`.

- **`src/plugin/mod.rs`** — `PluginRegistry`, `PluginEvent { event_id, event_type, correlation_id, circle_id, session_key, occurred_at, entity_id, entity_type, payload, actor_type, actor_id }`, `emit(event)`, `validate_plugin_event_namespace(plugin, event)` anti-spoofing.

### Files to modify

- **`src/state.rs`** — extend `AppState` with `harness_registry`, `plugin_registry`, `queue_backend: Arc<dyn QueueBackend>`, `generation_marker`, `kill_switch`.
- **`src/services/orchestrator.rs`** — refactor `run_turn()` to emit Submission and call `harness_dispatch::dispatch`. Remove direct `reasoning_loop::run()`.
- **`src/services/job_queue.rs`** — annotate `// TODO(P0-4): replace with sera-queue QueueBackend`; add QueueMode parameter stub.
- **`src/routes/chat.rs`, `src/routes/sessions.rs`, `src/routes/agents.rs`** — per §2.5 "no parallel RPC surface bypasses SQ/EQ": wrap each request as Submission(Op::UserTurn/Register/System) and forward to dispatch. Handlers become thin Submission emitters.

**Files to delete (Phase 0): None.** Route files retained as REST adapter wrappers. Direct `reasoning_loop` import removed from orchestrator.

### Cargo features

```toml
default = ["in-process"]
in-process = []
stdio = []
websocket = ["tokio-tungstenite"]
grpc = ["tonic", "prost"]
webhook-back = ["reqwest"]
connector-discord = []
enterprise = ["grpc", "webhook-back"]
```

### Workspace dep additions

```toml
tonic = { version = "0.12", optional = true }
prost = { version = "0.13", optional = true }
git2 = { version = "0.19", default-features = false }
async-trait = "0.1"
```

### Acceptance tests

| # | Test | Covers |
|---|------|--------|
| 1 | `envelope_submission_roundtrip` | Serialize/deserialize `Submission { op: Op::UserTurn }` |
| 2 | `envelope_event_roundtrip` | `Event { msg: EventMsg::StreamingDelta }` |
| 3 | `transport_enum_exhaustive` | Compile-time exhaustive match over `AppServerTransport` |
| 4 | `in_process_transport_dispatch` | mpsc round-trip |
| 5 | `stdio_transport_echo` | Child process NDJSON echo (feat-gated) |
| 6 | `websocket_transport_connect` | 127.0.0.1:0 connect + frame (feat-gated) |
| 7 | `harness_dispatch_routes_to_correct_harness` | Two MockAgentHarness instances, distinct AgentId |
| 8 | `session_persist_part_table_roundtrip` | sqlx in-memory write/read |
| 9 | `session_persist_snapshot_track_revert` | tempfile TempDir git snapshot |
| 10 | `kill_switch_admin_socket_accepts_shutdown` | UnixStream `ROLLBACK\n` → `OK\n`, state=Armed |
| 11 | `generation_marker_propagates_to_event_context` | Gateway GenerationMarker → Event ctx |
| 12 | `rest_chat_handler_wraps_as_submission` | POST handler → Submission on InProcess channel |

### Downstream cascade

- **sera-runtime** imports `AppServerTransport`; `main.rs` re-plumb uses `StdioTransport`. Add `sera-gateway` as dep (feature `stdio`).
- **sera-byoh-agent** defers rewrite to P2.
- **Future sera-sdk** will import envelope types as primary public API — plan stability.
- **sera-hooks** P1 — `PluginEvent` shape must match two-tier bus expectations; cross-check before finalising.

---

## P0-6 · sera-runtime contract migration

### Dependency note

Two hard sequencing dependencies:
1. **P0-1 (sera-types)** — `TurnOutcome`, `ContentBlock`, `ConversationMessage`, `ActionId`, `ChangeArtifactId`, `AgentCapability` must exist upstream.
2. **P0-5 (sera-gateway rename)** — `AppServerTransport::Stdio` must exist before `main.rs` re-plumb.

Correct order: sera-types → sera-hooks additive (if any) → sera-runtime contract migration → sera-gateway structural (or interleaved with coordination).

### Files to create / rewrite

- **`src/harness.rs`** — `AgentHarness` async trait (re-exported; definition in sera-types to avoid cycle). Methods per SPEC-runtime §3:
  ```rust
  fn supports(&self, ctx: &HarnessSupportContext) -> HarnessSupport;
  async fn run_attempt(&self, submission, ctx: EventContext) -> Result<impl Stream<Item=Event>, HarnessError>;
  async fn compact(&self, params: CompactionParams) -> Result<CompactionResult, HarnessError>;
  async fn reset(&self, params: ResetParams) -> Result<(), HarnessError>;
  async fn dispose(self: Box<Self>) -> Result<(), HarnessError>;
  ```
  `HarnessSupport { Supported | Unsupported { reason } | RequiresUpgrade { required_tier } }`. `DefaultHarness` concrete impl here.

- **`src/context/mod.rs`** — `ContextEngine` trait (separately pluggable axis per SPEC-runtime §2.4, orthogonal to `AgentRuntime`):
  ```rust
  async fn bootstrap(&mut self, agent: &AgentManifest) -> Result<(), ContextError>;
  async fn ingest(&mut self, msg: ConversationMessage) -> Result<(), ContextError>;
  async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError>;
  async fn compact(&mut self, trigger: CompactionTrigger) -> Result<CompactionCheckpoint, ContextError>;
  async fn maintain(&mut self) -> Result<(), ContextError>;
  async fn after_turn(&mut self, outcome: &TurnOutcome) -> Result<(), ContextError>;
  fn describe(&self) -> ContextEngineDescriptor;
  ```

- **`src/context/pipeline.rs`** — refactor `context_pipeline.rs` + `context_assembler.rs` into `ContextPipeline` impl of `ContextEngine`. Messages become `Vec<ConversationMessage>` (not flat ChatMessage). `ContextStep::stability_rank() -> u32` added (Phase 2 usage, field required).

- **`src/context/kvcache.rs`** — `KvCachePipeline` stub implementing `ContextEngine`; Phase 0 no-op reordering.

- **`src/compaction/mod.rs`** — `Condenser` async trait. `PipelineCondenser` holds `Vec<Box<dyn Condenser>>`. `CompactionCheckpoint { checkpoint_id, session_key, reason, pre_compaction, post_compaction, tokens_before, tokens_after, summary, created_at }`. `MAX_COMPACTION_CHECKPOINTS_PER_SESSION: u32 = 25`. `CheckpointReason { Manual, AutoThreshold, OverflowRetry, TimeoutRetry }`.

- **`src/compaction/condensers.rs`** — 9 Condenser impls per SPEC-runtime §6a:
  1. `NoOpCondenser` — passthrough
  2. `RecentEventsCondenser` — keep last N
  3. `ConversationWindowCondenser` — sliding window N turns, never splits ToolUse/ToolResult pairs
  4. `AmortizedForgettingCondenser` — probabilistic decay
  5. `ObservationMaskingCondenser` — mask tool results > threshold with `[MASKED: N bytes]`
  6. `BrowserOutputCondenser` — specialised for `browser_*` tool prefix
  7. `LLMSummarizingCondenser` — prose summary (P1 stub)
  8. `LLMAttentionCondenser` — relevance re-ranking (P1 stub)
  9. `StructuredSummaryCondenser` — JSON summary (P1 stub)

  LLM-backed ones feature-gated behind `llm-condensers`.

- **`src/handoff.rs`** — `Handoff<TContext>` first-class tool per SPEC-runtime §9b:
  ```rust
  pub struct Handoff<TContext> {
      pub tool_name: String,
      pub tool_description: String,
      pub input_json_schema: serde_json::Value,
      pub on_invoke_handoff: Box<dyn Fn(HandoffInputData<TContext>) -> BoxFuture<'static, TurnOutcome> + Send + Sync>,
      pub input_filter: Option<HandoffInputFilter>,
  }
  pub struct HandoffInputData<TContext> { input_history, pre_handoff_items, new_items, _ctx: PhantomData<TContext> }
  pub enum HandoffInputFilter { None, RemoveAllTools, Custom(...) }
  ```
  `Handoff::as_tool_definition() -> ToolDefinition`. When LLM invokes the handoff tool, `_act` calls `on_invoke_handoff` → returns `TurnOutcome::Handoff`.

- **`src/turn.rs`** — `TurnContext` + four-method turn loop:
  ```rust
  pub struct TurnContext {
      pub turn_id: Uuid,
      pub session_key: SessionKey,
      pub agent_id: AgentId,
      pub messages: Vec<ConversationMessage>,
      pub tools: Vec<ToolDefinition>,
      pub handoffs: Vec<Handoff<()>>,
      pub watch_signals: HashSet<ActionId>,
      pub change_artifact: Option<ChangeArtifactId>,  // §4.6 obligation
      pub react_mode: ReactMode,
      pub doom_loop_count: u32,
  }
  ```
  Four methods:
  - `_observe(ctx) -> Vec<ConversationMessage>` — filter by `cause_by ∈ watch_signals ∪ None`
  - `_think(messages, tools, react_mode, llm) -> LlmResponse` — LLM call; `ReactMode::ByOrder` deterministic (stub in P0)
  - `_act(response, ctx) -> Vec<ConversationMessage>` — dispatch ToolUse blocks; match against Handoff tool names first; doom-loop check on `doom_loop_count >= DOOM_LOOP_THRESHOLD` → Interruption
  - `_react(tool_results, ctx) -> TurnOutcome` — RunAgain/FinalOutput/Compact/Stop decision

  `DOOM_LOOP_THRESHOLD: u32 = 3`.

- **`src/subagent.rs`** — `SubagentHandle { agent_id, session_key, transport }`. `spawn_subagent` returns `Err(NotImplemented)` stub. Required because `Agent<TContext>.handoffs` references it.

- **`src/main.rs` REWRITE** — replace MVS `TaskInput`/`TaskOutput` with `AppServerTransport::Stdio` loop:
  1. Init tracing + RuntimeConfig
  2. Construct `DefaultHarness` from manifest path (env)
  3. Construct `StdioTransport` from sera-gateway (feat `stdio`)
  4. Loop: read Submission → `run_attempt` → stream Events back
  5. Exit on `Op::System(SystemOp::Shutdown)` or stdin close

  Heartbeat URL changes to Submission emission. `TaskInput`, `TaskOutput`, flat `ChatMessage`, `read_task_input`/`write_task_output` **deleted**. Old `reasoning_loop.rs`, `tool_loop_detector.rs`, `context_pipeline.rs`, `context_assembler.rs` deleted.

### Files to modify

- **`src/default_runtime.rs`** — `execute_turn` returns `TurnOutcome` (not `TurnResult`). Body calls `run_turn()` from `turn.rs`. Import: `use sera_types::runtime::TurnOutcome`.
- **`src/lib.rs`** — remove old modules; add harness/context/compaction/handoff/turn/subagent.
- **`src/session_manager.rs`** — `Vec<ChatMessage>` → `Vec<ConversationMessage>`.
- **`src/llm_client.rs`** — input/output `ConversationMessage`; local conversion layer for LiteLLM OpenAI wire format.
- **`src/heartbeat.rs`** — replace HTTP call with Submission emission.

**`TurnResult` grep sites to update:**
- `sera-domain/src/runtime.rs:68,159,211,336,347` (definition + tests)
- `sera-runtime/src/default_runtime.rs:10,92,118`
- `sera-core/src/bin/sera.rs:196` (local struct — evaluate whether to bridge or convert)

### Cargo features

```toml
default = ["stdio-binary"]
stdio-binary = ["sera-gateway/stdio"]
llm-condensers = []
kvcache-context = []
```

### Workspace dep additions

```toml
async-trait = "0.1"
tiktoken-rs = { version = "0.6", optional = true }   # behind llm-condensers; P0 uses char/4 estimate
```

### Acceptance tests

| # | Test | Asserts |
|---|------|---------|
| 1 | `turn_outcome_replaces_turn_result_compiles` | `TurnOutcome::RunAgain` assignable; fails if TurnResult still present |
| 2 | `context_engine_trait_object_safe` | `Box<dyn ContextEngine>` compiles |
| 3 | `content_block_roundtrip` | ToolUse JSON roundtrip |
| 4 | `conversation_message_cause_by_roundtrip` | cause_by preserved |
| 5 | `four_method_lifecycle_callable` | DefaultHarness + NoOpCondenser + mock LLM; _observe/_think/_act/_react sequence |
| 6 | `doom_loop_triggers_interruption` | doom_loop_count >= threshold → Interruption without tool dispatch |
| 7 | `handoff_dispatched_as_tool_call` | LLM returns ToolUse matching Handoff name → TurnOutcome::Handoff |
| 8 | `no_op_condenser_passthrough` | 5 messages in == 5 out |
| 9 | `conversation_window_condenser_retains_pairs` | No unpaired ToolUse in output |
| 10 | `pipeline_condenser_applies_in_order` | Chain produces expected state |
| 11 | `compaction_checkpoint_max_per_session` | const == 25 |
| 12 | `context_pipeline_wraps_as_context_engine` | `Box<dyn ContextEngine>` assign |
| 13 | `turn_context_has_change_artifact_field` | Compile-time §4.6 field |
| 14 | `each_condenser_compiles` | All 9 instantiate + `.name()` non-empty |
| 15 | `main_binary_boots_under_stdio_transport` | Spawn sera-runtime subprocess, NDJSON submission → event |

### Downstream cascade

- **sera-gateway** consumes `AgentHarness` trait. Resolution: define trait in **sera-types** to avoid cycle (gateway imports from sera-runtime would form one). Coordinate with Agent A to add to sera-types trait surface.
- **sera-hooks** P1 — `HookPoint::ConstitutionalGate` enforcement + `HookResult::updated_input` integration point stubs with `// TODO(P1)` comments at `_observe`/`_react` call sites.
- **sera-telemetry** P0-2 — `TurnHeartbeat { turn_id, status, tool_calls_completed, current_tool, elapsed, tokens_used }` defined in `turn.rs` with no-op emit stub; wiring is P0-2 responsibility.
## P0-9 · sera-workflow rewrite

### Strategy

Classified `needs-rewrite` (P0). Existing skeleton has `WorkflowDef`, `WorkflowTrigger`, `WorkflowRegistry`, dreaming config, cron-schedule types — but **no async executor, no task queue, no claim protocol, no persistence**. Existing `WorkflowRegistry` is synchronous and in-memory; cannot be the execution store.

Rewrite models `WorkflowTask` on the beads `Issue` schema (same semantics, content-hash identity, `Hooked` atomic-claim state). The `bd` CLI is **not** a Rust dep at any phase — shell-out integration behind `bd-shell` feature (HANDOFF §6.4).

Phase 0 output is the **type system + claim protocol** only. Runtime execution substrate (sqlx persistence, apalis job queue, circle coordination) is Phase 1.

Preserve as-is: `dreaming`, `schedule`, `session_key`. Modify or replace: `registry.rs`, `types.rs`, `lib.rs`.

### Files to create

#### `src/task.rs`

```
WorkflowTaskId          — SHA-256 content hash (not Uuid/String)
WorkflowTask            — beads Issue schema
WorkflowTaskStatus      — Open | InProgress | Hooked | Blocked | Deferred | Closed | Pinned
WorkflowTaskType        — Feature | Bug | Chore | Research | Meta | Dream
AwaitType               — GhRun | GhPr | Timer | Human | Mail | Change
DependencyType          — Blocks | Related | ParentChild | DiscoveredFrom | ConditionalBlocks
WorkflowTaskDependency  — { from, to, kind }
WorkflowSentinel        — Start | SelfLoop | Prev | Next | End | Named(String)
```

`WorkflowTask` fields:
- id, title, description, acceptance_criteria, status, priority (u8, 0=highest), task_type
- assignee: Option<PrincipalRef>, due_at, defer_until, metadata
- await_type, await_id, timeout (claim-reaper threshold)
- mol_type, work_type, ephemeral (deleted on Closed), wisp_type
- source_formula, source_location, created_at (all hashed into WorkflowTaskId)
- **`meta_scope: Option<BlastRadius>`** — §4.6 obligation
- **`change_artifact_id: Option<ChangeArtifactId>`** — §4.6 obligation

`WorkflowTaskId` is `[u8;32]`. Canonical hash input: pipe-delimited UTF-8 of title | description | first acceptance-criterion | source_formula | source_location | created_at(RFC-3339), SHA-256 via `sha2`. Display/FromStr as hex.

`WorkflowTaskStatus::Hooked` is the **atomic-claim intermediate state**. `Open → Hooked` during claim; claimant promotes to `InProgress` on setup. Collapsing Hooked into InProgress loses crash-recovery claim window.

`DependencyType::ConditionalBlocks` = "B runs only if A fails." First-class primitive — not generic metadata.

#### `src/ready.rs`

Pure-function port of `bd ready` algorithm (SPEC-workflow-engine §4a):

```rust
fn ready_tasks(tasks: &[WorkflowTask], now: DateTime<Utc>) -> Vec<&WorkflowTask>
```

Five gates:
1. `status == Open`
2. No `Blocks | ConditionalBlocks` dependency where source has `status ∈ {Open, InProgress, Hooked, Blocked}`. ConditionalBlocks exception: if blocker is `Closed`, edge is satisfied (A failed → B unblocked).
3. `defer_until <= now` or None
4. `await_type.is_none()` (Phase 0 treats any present await_type as blocking; AwaitResolver is P1)
5. Not `ephemeral && status == Closed`

Sort by (priority ASC, id) for determinism.

Also: `dependency_closure(tasks, roots) -> Vec<WorkflowTaskId>` (topological test helper).

#### `src/claim.rs`

Atomic claim protocol (SPEC-workflow-engine §4b):

```rust
pub struct ClaimToken {
    pub task_id: WorkflowTaskId,
    pub agent_id: String,
    pub claimed_at: DateTime<Utc>,
    pub idempotency_key: uuid::Uuid,
}

pub enum ClaimError {
    StatusMismatch { current: WorkflowTaskStatus },
    AlreadyClaimed { by: String },
    NotFound,
    StorageError(String),
}

pub fn claim_task(tasks: &mut Vec<WorkflowTask>, task_id: &WorkflowTaskId, agent_id: &str, now: DateTime<Utc>) -> Result<ClaimToken, ClaimError>
pub fn confirm_claim(tasks: &mut Vec<WorkflowTask>, token: &ClaimToken) -> Result<(), ClaimError>
```

In-memory impl: compare-and-swap on status (only transitions if still Open). Phase 1 replaces with `SELECT ... FOR UPDATE` + `UPDATE ... WHERE status='open' RETURNING` in sqlx txn. `ClaimToken` travels unchanged into P1.

`StaleClaimReaper::reap_stale(tasks, now)` stub: iterate Hooked tasks, reset to Open where `claimed_at + timeout < now`. Full background wiring in P1.

#### `src/termination.rs`

```rust
pub struct TerminationConfig { max_rounds: Option<u32>, max_cost_usd: Option<f64> }
pub struct TerminationState { rounds_elapsed: u32, cost_usd_accumulated: f64, consecutive_idle_rounds: u32 }
pub enum TerminationReason {
    NRoundExceeded { limit: u32 },
    Idle { consecutive_rounds: u32 },
    BudgetExhausted { limit_usd: f64, actual_usd: f64 },
    ExplicitStop,
}
pub struct WorkflowTermination { reason: TerminationReason, terminated_at: DateTime<Utc> }
pub fn check_termination(config: &TerminationConfig, state: &TerminationState) -> Option<TerminationReason>
```

`Idle` fires when `consecutive_idle_rounds >= 3`. Any one condition fires termination; all three checked each round.

#### `src/cron.rs`

```rust
#[cfg(feature = "cron")]
pub mod cron {
    pub struct CronJobHandle { workflow_name, expression, next_fire }
    pub enum CronError { InvalidExpression(String), RegistrationFailed(String) }
    pub fn register_cron_workflow(def: &WorkflowDef) -> Result<CronJobHandle, CronError>
}
```

Validates cron expression via existing `cron` crate. Full apalis worker instantiation is P1.

#### `src/bd_shell.rs`

```rust
#[cfg(feature = "bd-shell")]
pub mod bd_shell {
    pub enum BdShellError { NotFound, ExitNonZero { code, stderr }, ParseError(String) }
    pub async fn bd_ready() -> Result<Vec<String>, BdShellError>
    pub async fn bd_claim(id: &str) -> Result<(), BdShellError>
    pub async fn bd_close(id: &str) -> Result<(), BdShellError>
}
```

Phase 0: all stubs return `Err(NotFound)`. Real `tokio::process::Command` wiring in P1.

#### `src/lib.rs` (modified)

```rust
pub mod claim;
pub mod cron;
pub mod dreaming;  // preserved
pub mod error;
pub mod ready;
pub mod registry;  // #[deprecated] in favour of WorkflowEngine (P1)
pub mod schedule;  // preserved
pub mod session_key;  // preserved
pub mod task;
pub mod termination;

#[cfg(feature = "bd-shell")]
pub mod bd_shell;

// Re-exports: task types, claim types, ready fn, termination types, existing types from types.rs
```

### Files to modify

| File | Change |
|------|--------|
| `src/types.rs` | `WorkflowDef.agent_id: String` → `PrincipalRef`; add `hook_chain: Option<String>`, `step_sentinels: bool` |
| `src/error.rs` | Variants: `ClaimFailed(ClaimError)`, `TaskNotFound(WorkflowTaskId)`, `BudgetExhausted { limit_usd }`, `NRoundExceeded { limit }` |
| `src/registry.rs` | `#[deprecated(note = "Use WorkflowEngine (Phase 1)")]` |
| `Cargo.toml` | Features + deps (below) |

### Cargo features

```toml
[features]
default = []
bd-shell = ["dep:tokio"]
cron = ["dep:apalis"]

[dependencies]
sera-types = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true, features = ["v4"] }
chrono = { workspace = true, features = ["serde"] }
cron = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }

apalis = { workspace = true, optional = true }
tokio = { workspace = true, optional = true, features = ["process"] }
```

### Workspace dep additions

```toml
sha2 = "0.10"
hex = "0.4"
# apalis already pinned by P0-4
```

### Acceptance tests

| # | Test | Location | Asserts |
|---|------|----------|---------|
| 1 | `workflow_task_id_content_hash_stable` | `task.rs` | Same fields → same hash, order-independent |
| 2 | `workflow_task_id_hex_roundtrip` | `task.rs` | Display → FromStr roundtrip |
| 3 | `workflow_task_id_differs_on_title_change` | `task.rs` | Title change → different hash |
| 4 | `ready_tasks_no_open_blockers` | `ready.rs` | Closed blocker via Blocks → task ready |
| 5 | `ready_tasks_conditional_blocks_satisfied_when_blocker_closed` | `ready.rs` | ConditionalBlocks + Closed source → ready |
| 6 | `ready_tasks_conditional_blocks_not_satisfied_when_blocker_open` | `ready.rs` | ConditionalBlocks + Open source → blocked |
| 7 | `ready_tasks_defer_until_future` | `ready.rs` | defer_until filter |
| 8 | `ready_tasks_sorted_by_priority` | `ready.rs` | Priority ASC ordering |
| 9 | `atomic_claim_transitions_to_hooked` | `claim.rs` | Open → Hooked + token |
| 10 | `double_claim_returns_already_claimed` | `claim.rs` | Second call → AlreadyClaimed |
| 11 | `workflow_task_status_hooked_to_inprogress_via_confirm` | `claim.rs` | confirm_claim → InProgress |
| 12 | `termination_triad_fires_on_each_condition` | `termination.rs` | Three checks — n_round, idle, budget — each returns Some correctly |
| 13 | `meta_scope_blast_radius_serde_roundtrip` | `task.rs` | WorkflowTask with meta_scope preserved through serde |
| 14 | `stale_claim_reaper_resets_timed_out_hooked_tasks` | `claim.rs` | T+2s reaper resets status |

### Downstream cascade

Phase 0 = types only.

- **sera-gateway** (P1): imports `WorkflowTask`, `ready_tasks`, `ClaimToken` for work dispatch. No gateway code change in P0.
- **sera-runtime** (P1): receives `ClaimToken` in turn context; presents on `confirm_claim`. No runtime code change in P0.
- **sera-hitl** (P1): `meta_scope: Some(BlastRadius)` routes through constitutional gate + HITL. `change_artifact_id` links to `ApprovalScope::ChangeArtifact`.
- **Circle coordination** (P1+): `ClaimToken`/`ClaimError` shared with future circle claim ops. Types must not be renamed.

---

## P0-7 · sera-auth design-forward types + feature gates

### Strategy

Classified `needs-extension` (P0). Two blocking gaps:
1. **Security defect** (§4.7): `StoredApiKey.key_hash` compared plaintext `==` at `api_key.rs:35` ("plaintext comparison during alpha"). Must replace with argon2 before `basic-auth` declared stable.
2. **Design-forward gap** (§4.6): `AgentCapability`, `CapabilityToken`, `Action::ProposeChange`, `Action::ApproveChange`, `Resource::ChangeArtifact` must exist as typed stubs.

Also wire casbin 2.19 as RBAC backend for `DefaultAuthzProvider` (replace `Allow`-everything placeholder) and install the `[features]` block from SPEC-crate-decomposition §6.2.

**Not a rewrite.** Existing `Action`, `Resource`, `AuthzDecision`, `AuthorizationProvider`, `JwtService`, middleware preserved.

### Files to create

#### `src/capability.rs`

```rust
use sera_types::evolution::{AgentCapability, BlastRadius, ChangeArtifactId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub token_id: uuid::Uuid,
    pub agent_id: String,
    pub capabilities: HashSet<AgentCapability>,
    pub blast_radius: Option<BlastRadius>,
    pub proposals_consumed: u32,
    pub max_proposals: Option<u32>,
    pub revocation_check_required: bool,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum CapabilityTokenError {
    CapabilityMissing(AgentCapability),
    WideningAttempt,
    Expired,
    ProposalLimitExhausted { limit: u32, consumed: u32 },
}

impl CapabilityToken {
    pub fn narrow(&self, capabilities: HashSet<AgentCapability>, blast_radius: Option<BlastRadius>) -> Result<CapabilityToken, CapabilityTokenError>;
    pub fn has(&self, cap: AgentCapability) -> bool;
    pub fn consume_proposal(&mut self) -> Result<(), CapabilityTokenError>;
}
```

Narrowing rule: requested capabilities ⊆ self.capabilities; requested blast_radius ⪯ self.blast_radius (natural severity ordering from sera-types). Fail → `WideningAttempt`.

Note: `AgentCapability` enum is defined in sera-types (per Agent A plan) and imported here.

### Files to modify

#### `src/api_key.rs` — argon2 replacement

Rename `StoredApiKey.key_hash` → `key_hash_argon2: String` (PHC string). `ApiKeyValidator::validate` calls `argon2::PasswordHash::new(&k.key_hash_argon2)` then `Argon2::default().verify_password(token.as_bytes(), &parsed_hash)`. Add `AuthError::HashingError(String)` for malformed stored hash. Test helper `fn hash_key(raw: &str) -> String` using `Argon2::default().hash_password(raw, &SaltString::generate(&mut OsRng))`. **No fallback, no `#[cfg(test)]` bypass** — plaintext path entirely removed.

`BasicAuthValidator` referenced in audit does not exist as distinct struct; `ApiKeyValidator` is the correct locus.

#### `src/authz.rs` — new variants

Add to `Action`:
```rust
ProposeChange(BlastRadius),
ApproveChange(ChangeArtifactId),
```

Add to `Resource`:
```rust
ChangeArtifact(ChangeArtifactId),
```

`AuthzDecision::NeedsApproval` payload: audit says `ApprovalSpec` but that lives in sera-hitl → would create cycle. Resolution: define lightweight `PendingApprovalHint { routing_hint: String, scope: Option<String> }` in sera-auth for Phase 0. Phase 1 upgrades after cycle is resolved.

`DefaultAuthzProvider` stub for new actions — deny by default until casbin is wired:
```rust
Action::ProposeChange(_) | Action::ApproveChange(_) => Ok(AuthzDecision::Deny(DenyReason::new(
    "capability_required",
    "ProposeChange/ApproveChange require MetaChange/MetaApprover capability token",
)))
```

#### `src/authz/casbin_adapter.rs` (new)

```rust
pub struct CasbinAuthzAdapter { enforcer: casbin::Enforcer }

impl CasbinAuthzAdapter {
    pub async fn from_strings(model_text: &str, policy_text: &str) -> Result<Self, casbin::Error>;
    pub async fn enforce(&self, subject: &str, object: &str, action: &str) -> Result<bool, casbin::Error>;
}
```

`CasbinAuthzProvider` wraps adapter, implements `AuthorizationProvider`. Maps `PrincipalRef.id` → subject, `Resource` display → object, `Action` display → action. `ChangeArtifact` and `ProposeChange`/`ApproveChange` map to natural string forms.

#### `Cargo.toml`

```toml
[features]
default = ["jwt", "basic-auth"]
jwt = []
basic-auth = ["dep:argon2"]
enterprise = ["oidc", "scim", "authzen", "ssf"]
oidc = ["dep:openidconnect", "dep:oauth2", "dep:axum-login", "dep:tower-sessions"]
scim = ["dep:scim-server"]
authzen = []
ssf = []

[dependencies]
sera-types = { workspace = true }
sera-db.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
thiserror.workspace = true
jsonwebtoken.workspace = true
axum.workspace = true
async-trait = { workspace = true }

# NEW (always on)
casbin = { workspace = true }
uuid = { workspace = true, features = ["v4"] }
chrono = { workspace = true, features = ["serde"] }

# NEW (feature-gated)
argon2 = { workspace = true, optional = true }
openidconnect = { workspace = true, optional = true }
oauth2 = { workspace = true, optional = true }
scim-server = { workspace = true, optional = true }
axum-login = { workspace = true, optional = true }
tower-sessions = { workspace = true, optional = true }
```

casbin is always on (replaces placeholder). argon2 pulled in by `basic-auth` in default.

#### `src/lib.rs`

```rust
pub mod api_key;
pub mod authz;
pub mod capability;   // new
pub mod error;
pub mod jwt;
pub mod middleware;
pub mod types;

pub use capability::{CapabilityToken, CapabilityTokenError};
// AgentCapability re-exported from sera-types
pub use authz::{..., CasbinAuthzProvider};
```

### Workspace dep additions

```toml
casbin = "2.19"
argon2 = "0.5"
# Enterprise tier P2+ — commented for documentation:
# openidconnect = "3.5"
# oauth2 = "5"
# scim-server = "0.5"
```

### Acceptance tests

| # | Test | Location | Asserts |
|---|------|----------|---------|
| 1 | `capability_token_narrowing_removes_capability` | `capability.rs` | {MetaChange,CodeChange} → {CodeChange} succeeds |
| 2 | `capability_token_narrowing_widening_denied` | `capability.rs` | {CodeChange} → {MetaChange,CodeChange} → WideningAttempt |
| 3 | `capability_token_has_returns_correct_results` | `capability.rs` | has() true/false correctness |
| 4 | `capability_token_proposal_limit_enforced` | `capability.rs` | max=2: 2 OK, 3rd ProposalLimitExhausted |
| 5 | `agent_capability_enum_exhaustive_serde` | `capability.rs` | All variants roundtrip |
| 6 | `propose_change_action_roundtrip_with_blast_radius` | `authz.rs` | Action::ProposeChange(SessionLocal) serde |
| 7 | `casbin_rbac_allows_authorised_subject` | `casbin_adapter.rs` | Policy-granted enforce → true |
| 8 | `casbin_rbac_denies_unauthorised_subject` | `casbin_adapter.rs` | Non-granted → false |
| 9 | `casbin_change_artifact_policy_evaluation` | `casbin_adapter.rs` | meta-approver + change-artifact/* pattern |
| 10 | `argon2_password_verify_positive` | `api_key.rs` | Correct plaintext verifies |
| 11 | `argon2_password_verify_negative` | `api_key.rs` | Wrong plaintext → Unauthorized |
| 12 | `plaintext_comparison_path_absent` | `api_key.rs` | `include_str!("api_key.rs").contains("key_hash ==")` == false |

Plus CI check: `cargo check -p sera-auth --no-default-features` and `--features enterprise` both pass.

### Downstream cascade

- **sera-gateway** P1: imports `Action::ProposeChange`/`ApproveChange`, `Resource::ChangeArtifact` for Submission authz. Calls `AuthorizationProvider::check` with CapabilityToken-derived capabilities before enqueuing.
- **sera-hitl** P1: `ApprovalScope::ChangeArtifact(ChangeArtifactId)` + `MetaChangeContext` with approver-pinning. Shared `ChangeArtifactId` via sera-types.
- **sera-workflow** (same phase P0-9): `WorkflowTask.meta_scope`/`change_artifact_id` use same sera-types primitives. Both crates depend on sera-types, not each other.
- **Phase 2+ enterprise**: `oidc`/`scim` features compile independently — CI verifies before Phase 2 work.
## Acceptance-test catalog — design-forward obligations (§4.6)

This catalog is the traceability matrix between the 23 design-forward obligations listed in IMPL-AUDIT.md §4.6 and the Phase 0 acceptance test suite drafted across Wave 1 plans (agents A–E). Each obligation must have at least one passing test before M4 (full Phase 0 gate) is declared complete. Any obligation whose "Test fn" cell reads `GAP` is a blocking deficiency: the test does not exist in any Wave 1 plan and must be added before M4.

| § | §4.6 obligation | Target crate | Test fn | File | Assertion |
|---|----------------|--------------|---------|------|-----------|
| 1 | `ChangeArtifactId` in `sera-domain` (→ sera-types) | `sera-types` | `change_artifact_id_display_is_hex` | `tests/evolution.rs` | `[0u8;32]` Display produces 64-char lowercase hex; type exists and is hashable |
| 2 | `BlastRadius` enum (22 variants) in `sera-domain` (→ sera-types) | `sera-types` | `blast_radius_has_22_variants` | `tests/evolution.rs` | Exhaustive match with `#[deny(unreachable_patterns)]` — exactly 22 arms compile |
| 3 | `CapabilityToken` with narrowing rule in `sera-domain` + `sera-auth` | `sera-auth` | `capability_token_narrowing_removes_capability` | `tests/capability.rs` | `{MetaChange,CodeChange}` narrowed to `{CodeChange}` succeeds; also `capability_token_narrowing_widening_denied` verifies widening is rejected |
| 4 | `ConstitutionalRule` in `sera-domain` (→ sera-types) | `sera-types` | `GAP` | — | No Wave 1 test covers `ConstitutionalRule` struct existence or serde roundtrip |
| 5 | `EvolutionTier` in `sera-domain` (→ sera-types) | `sera-types` | `evolution_tier_non_exhaustive_serde` | `tests/evolution.rs` | All 3 variants (`AgentImprovement`/`ConfigEvolution`/`CodeEvolution`) roundtrip through JSON |
| 6 | `AgentCapability` enum with `MetaChange`/`CodeChange`/`MetaApprover` in `sera-domain` + `sera-auth` | `sera-types` | `agent_capability_all_variants_serde` | `tests/evolution.rs` | All 5 variants roundtrip as snake_case JSON; `sera-auth` side covered by `agent_capability_enum_exhaustive_serde` in `tests/capability.rs` |
| 7 | `BuildIdentity` in `sera-domain` (→ sera-types) | `sera-types` | `build_identity_serde_roundtrip` | `tests/versioning.rs` | All fields (`version`, `commit`, `build_time`, `signer_fingerprint`, `constitution_hash`) preserved through JSON roundtrip |
| 8 | `ResourceMetadata.change_artifact: Option<ChangeArtifactId>` + `.shadow: bool` | `sera-types` | `resource_metadata_shadow_field_defaults_false` | `tests/config_manifest.rs` | `shadow` field defaults to `false`; `change_artifact` field exists on the struct |
| 9 | `PersonaSpec.mutable_persona` + `.mutable_token_budget` | `sera-types` | `persona_spec_mutable_fields_round_trip` | `tests/config_manifest.rs` | Both optional fields survive YAML roundtrip with and without values set |
| 10 | `SessionState::Shadow` variant | `sera-types` | `shadow_session_valid_transitions` | `tests/session.rs` | `Created→Shadow→Destroyed` accepted; `Shadow→Active` rejected by `can_transition_to` |
| 11 | `HookPoint::ConstitutionalGate` + fail-closed enforcement in executor | `sera-types` | `GAP` | — | No Wave 1 test proves `ConstitutionalGate` is fail-closed (enforcement stub is a P1 `// TODO` comment in sera-runtime; no test verifies the fail-closed default) |
| 12 | `HookContext::change_artifact: Option<ChangeArtifactId>` | `sera-types` | `GAP` | — | No Wave 1 test verifies the `change_artifact` field on `HookContext`; hook struct tests are not present in any agent plan |
| 13 | `HookResult::updated_input` | `sera-types` | `GAP` | — | No Wave 1 test covers the `updated_input: Option<serde_json::Value>` field on `HookResult::Continue` |
| 14 | `TurnContext::change_artifact: Option<ChangeArtifactId>` | `sera-runtime` | `turn_context_has_change_artifact_field` | `tests/turn.rs` | Compile-time check that `TurnContext.change_artifact` field of type `Option<ChangeArtifactId>` exists (§4.6 named obligation) |
| 15 | `ConversationMessage.cause_by: Option<ActionId>` (required for `_observe` watch_signals filtering) | `sera-types` | `conversation_message_cause_by_roundtrip` | `tests/content_block.rs` | `cause_by` field present and preserved through JSON; `sera-runtime` side covered by `conversation_message_cause_by_roundtrip` in `tests/context.rs` |
| 16 | `WorkflowTask.meta_scope: Option<BlastRadius>` + `.change_artifact_id` | `sera-workflow` | `meta_scope_blast_radius_serde_roundtrip` | `tests/task.rs` | `WorkflowTask` with `meta_scope: Some(BlastRadius::…)` and `change_artifact_id` fields survives serde roundtrip |
| 17 | `ApprovalScope::ChangeArtifact` + `::MetaChange` + `MetaChangeContext` with approver pinning | `sera-auth` | `GAP` | — | No Wave 1 test covers `ApprovalScope::ChangeArtifact`/`MetaChange` or `MetaChangeContext`; agent-e notes these are P1 (sera-hitl), but the types must exist as stubs in Phase 0 |
| 18 | `Action::ProposeChange(BlastRadius)` + `::ApproveChange(ChangeArtifactId)` + `Resource::ChangeArtifact` | `sera-auth` | `propose_change_action_roundtrip_with_blast_radius` | `tests/authz.rs` | `Action::ProposeChange(BlastRadius::SessionLocal)` serialises and deserialises correctly; `Resource::ChangeArtifact` existence is implicitly verified by the casbin policy test |
| 19 | `ShadowConfigStore` overlay type (even as empty stub) | `sera-config` | `shadow_overlay_shadows_prod_value` | `tests/test_shadow_store.rs` | Overlay value takes precedence over prod value; fallthrough and discard covered by two additional tests in the same file |
| 20 | `ConfigVersionLog` append-only skeleton | `sera-config` | `version_log_starts_empty` | `tests/test_version_log.rs` | Empty log has version 0 and zero tail hash; chain linkage (`prev_hash` ← `this_hash`) verified by `version_log_prev_hash_chain_is_linked` |
| 21 | Separate audit write path (`AuditBackend` trait + `OnceCell` static binding) | `sera-telemetry` | `audit_backend_set_twice_panics` | `tests/test_audit_set_once.rs` | Double-set of `AUDIT_BACKEND` panics; `audit_append_returns_not_initialised_when_backend_unset` verifies structural isolation from the `Emitter` path |
| 22 | Kill-switch admin socket + CON-04 boot-time health check | `sera-tools` | `kill_switch_boot_health_check_fails_closed` | `tests/kill_switch.rs` | Bad socket path returns `Err`, no panic; `con04_boot_check_passes_with_valid_socket` verifies tempdir path returns `Ok`; `kill_switch_bind_and_receive_rollback` verifies `ROLLBACK` command round-trip |
| 23 | `GenerationMarker` on `EventContext` | `sera-gateway` | `generation_marker_propagates_to_event_context` | `tests/envelope.rs` | Gateway `GenerationMarker` is copied into every `EventContext`; field presence verified at compile time by `generation_marker_round_trips_json` in `sera-telemetry/tests/test_generation_marker.rs` |

---

### Gaps

Four §4.6 obligations have no matching test in any Wave 1 plan. All four block M4.

| § | Obligation | Recommendation |
|---|-----------|----------------|
| 4 | `ConstitutionalRule` in sera-types | Add `constitutional_rule_serde_roundtrip` to `rust/crates/sera-types/tests/evolution.rs`. Verify all four fields (`id`, `description`, `enforcement_point`, `content_hash`) survive JSON roundtrip and that `ConstitutionalEnforcementPoint` has 4 variants. |
| 11 | `HookPoint::ConstitutionalGate` + fail-closed enforcement | Add `hook_point_constitutional_gate_is_fail_closed` to `rust/crates/sera-types/tests/hooks.rs` (new file). Assert that `HookPoint::ConstitutionalGate.default_behavior()` (or equivalent sentinel) returns the fail-closed value, and that `HookPoint::ALL.len() == 20`. |
| 12 | `HookContext::change_artifact: Option<ChangeArtifactId>` | Add `hook_context_change_artifact_field_roundtrip` to `rust/crates/sera-types/tests/hooks.rs`. Construct `HookContext` with `change_artifact: Some(ChangeArtifactId([1u8;32]))`, JSON-roundtrip, and assert field is preserved. |
| 13 | `HookResult::updated_input` | Add `hook_result_updated_input_roundtrip` to `rust/crates/sera-types/tests/hooks.rs`. Construct `HookResult::Continue { updated_input: Some(json!({"rewritten": true})) }`, JSON-roundtrip, and assert field is preserved. |

---

### Coverage by crate

| Crate | §4.6 obligations covered | Total Wave 1 tests |
|---|---|---|
| `sera-types` | §1, §2, §5, §6, §7, §8, §9, §10 (+ gaps §4, §11, §12, §13) | 15 |
| `sera-auth` | §3, §6 (narrowing), §18 | 12 |
| `sera-runtime` | §14, §15 (runtime side) | 15 |
| `sera-workflow` | §16 | 14 |
| `sera-config` | §19, §20 | 14 (shadow: 3, version_log: 5, schema: 1, env: 2, layer: 3) |
| `sera-telemetry` | §21, §23 (marker type) | 14 |
| `sera-tools` | §22 | 15 |
| `sera-gateway` | §23 (propagation) | 12 |

# SERA Rust Migration — Implementation Tracker

> **Document Status:** Current (Updated 2026-04-17 — Session 26)
> **Purpose:** Master tracking document for SERA 2.0 Rust migration
> **Basis:** Full spec analysis + codebase inspection + test run verification

---

## 1. Executive Summary

### Current State Overview

The SERA Rust workspace is **fully scaffolded** with **all 27 planned crates** present and building. The workspace compiles successfully and all tests pass (2,455 tests across 27 crates).

| Metric | Value |
|--------|-------|
| Total Crates Planned | 27 |
| Crates in Workspace | 27 |
| Missing Crates | None |
| Total Rust LOC | ~168,781 (376 .rs files) |
| Build Status | ✅ COMPILES (release build passing) |
| Test Status | ✅ ALL PASSING (2,455 tests) |

### Phase Completion

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| Phase 0 | Foundation (types, config, DB, queue, telemetry, errors, cache, secrets) | ✅ COMPLETE | 100% |
| Phase 1 | Core Domain (session, auth, tools, hooks, workflow, models, skills) | COMPLETE | 95% |
| Phase 2 | Runtime & Gateway (runtime, gateway, TUI, BYOH, meta) | IN PROGRESS | 97% |
| Phase 3 | Interop Protocols (MCP, A2A, AG-UI, plugins) | IN PROGRESS | 75% |
| Phase 4 | Enterprise & Hardening (OIDC/SCIM, K8s, Circles full) | NOT STARTED | 0% |

### Key Achievements (Session 15b verified; Sessions 25–26 extended)

1. **Core gateway operational** — `sera-gateway` with 81 source files, 21,757 LOC, 270+ tests; startup validation hardened
2. **Runtime infrastructure complete** — `sera-runtime` with 37 source files, 8,180 LOC, 106+ tests; ToolUseBehavior runtime enforcement landed
3. **Model provider abstractions created** — `sera-models` (219 LOC) with `ModelProvider` trait; 62 tests (was wrongly reported as 0)
4. **Skill pack loading created** — `sera-skills` (349 LOC) with filesystem-based discovery; 112 tests across 6 modules
5. **Self-evolution machinery complete** — `sera-meta` (2,196 LOC) with 3-tier policy, shadow sessions, constitutional rules; ArtifactPipeline wired
6. **HITL approval production-ready** — `sera-hitl` (819 LOC) with full escalation chains and tests
7. **Workflow engine comprehensive** — `sera-workflow` (3,145 LOC) with SCC cycle detection, termination detection, coordination; Timer gate (`AwaitType::Timer`) landed
8. **Type system comprehensive** — `sera-types` (8,921 LOC) with 29 modules covering full domain; NDJSON ProtocolCapabilities + HandshakeFrame finalized
9. **Hooks hardened** — `sera-hooks` PermissionOverrides + HookCancellation + `updated_input` transformation landed; WASM adapter feature-gated via `wasmtime`
10. **Session 25 ultrawork marathon** — 16 beads closed, ~95 new tests, gateway stub classification complete, HybridScorer (586 LOC, 14 tests) production-ready
11. **Session 26 ultrawork marathon** — ~20 beads closed, ~366 new tests across 20 crates; RoleBasedAuthzProvider (Tier-1.5), ToolUseBehavior enforcement, commit_overlay bugfix, llm_proxy JWT impersonation fix, Timer gate, PermissionOverrides+HookCancellation, BYOH build_* seam extraction, contracts.rs golden YAML harness

### Remaining Gaps

1. **sera-gateway HTTP routes** — Artifact proposal/evaluate/approve/apply handlers not yet wired (post-sera-k2gw)
2. **HookContext.change_artifact threading** — Integration gap in gateway pipeline (follow-up to ser-k2gw)
3. **Session commands schema** — task_queue.session_id parent key alignment (noted in gateway-stubs-classification.md)
4. **Interop crates** — sera-mcp, sera-a2a, sera-agui, sera-plugins at ~75% (Phase 3, in progress)
5. **Clippy compliance** — Workspace passes `cargo clippy -- -D warnings` (Session 21+); 56% dead code reduction (Session 25)
6. **WASM fuel metering** — `sera-hooks` fuel + memory caps not yet configured
7. **Circles coordination** — 7-policy implementation, blackboard, convergence incomplete (~40%)

---

## 2. Per-Crate Status

### Foundation Crates (Phase 0)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-types | ✅ COMPLETE | 8,921 | 334 | 29 modules, comprehensive domain types |
| sera-config | ✅ COMPLETE | 2,129 | 81 | Layered config, schema registry, file watcher; commit_overlay bugfix landed (S26) |
| sera-errors | ✅ COMPLETE | 248 | 5 | SeraErrorCode, SeraError, IntoSeraError trait; wired into gateway + runtime |
| sera-cache | ✅ COMPLETE | 134 | 22 | MokaBackend with full test suite; +15 tests (S26); Redis deferred to Phase 4 |
| sera-db | ✅ COMPLETE | 3,836 | 79 | PostgreSQL (sqlx) + SQLite (rusqlite), 21 source files |
| sera-queue | ✅ COMPLETE | 470 | 23 | QueueBackend trait, local + apalis backends; +12 tests (S26) |
| sera-telemetry | ✅ COMPLETE | 436 | 91 | OTel triad (version-pinned), AuditBackend, OCSF; +45 tests (S26) |
| sera-secrets | ✅ COMPLETE | 636 | 53 | Env, Docker, File, Chained providers + enterprise scaffolds; +31 tests (S26) |

### Core Domain Crates (Phase 1)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-session | ✅ COMPLETE | 1,391 | 78 | 6-state machine, transcript, 4-tier memory |
| sera-auth | ✅ COMPLETE | 1,289 | 53 | JWT, OIDC, API keys, casbin RBAC; RoleBasedAuthzProvider Tier-1.5 landed (S26) |
| sera-tools | ✅ COMPLETE | 1,900+ | 145 | 5 sandbox providers, SSRF, bash AST, kill switch, contradiction detection, source mounts |
| sera-hooks | ✅ COMPLETE | 1,206 | 37 | Native hooks + WASM adapter; PermissionOverrides + HookCancellation + updated_input (S26) |
| sera-hitl | ✅ COMPLETE | 819 | 20 | Full approval workflow, escalation chains, tests in lib |
| sera-workflow | ✅ COMPLETE | 3,145 | 110 | Atomic claims, SCC cycle detection, termination, coordination; Timer gate landed (S26) |
| sera-events | ✅ COMPLETE | 501 | 34 | Audit Merkle chain (SHA-256), Centrifugo pub/sub; +22 tests (S26) |
| sera-models | ✅ COMPLETE | 219 | 75 | ModelProvider trait, ProviderConfig, ModelResponse (was wrongly reported as 0) |
| sera-skills | ✅ COMPLETE | 880+ | 190 | SkillLoader, SkillPack trait, YAML discovery, KnowledgeSchemaValidator; +50 tests (S26) |

### Runtime & Gateway (Phase 2)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-runtime | ⚠️ 96% | 8,180 | 256 | Core operational; all 9 condensers implemented; ToolUseBehavior enforcement landed (S26) |
| sera-gateway | ⚠️ 92% | 21,757 | 391 | Core operational; OIDC seam + intercom acting-context + llm_proxy JWT priority (S26) |
| sera-meta | ✅ COMPLETE | 2,196 | 72 | 3-tier evolution, shadow sessions, constitutional rules (Epic 30 P2 closed) |
| sera-tui | ✅ COMPLETE | 835 | 67 | ratatui TUI, crossterm input; +50 tests (S26) |
| sera-byoh-agent | ✅ COMPLETE | 221 | 52 | BYOH reference implementation; build_* seam extraction + 52 tests (S26) |
| sera-testing | ✅ COMPLETE | 326 | 37 | Mock implementations, contracts.rs golden YAML harness (S26); +29 tests |

### Interop & Plugin Crates (Phase 3) — Added Sessions 19-20

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-mcp | ✅ IN PROGRESS | — | 70 | MCP server/client bridge; gating + rmcp_bridge + errors; +19 tests (S26) |
| sera-a2a | ✅ IN PROGRESS | — | 15 | A2A protocol adapter; Client + InProcRouter + Capabilities; +7 tests (S26) |
| sera-agui | ✅ IN PROGRESS | — | 17 | AG-UI streaming protocol, 17 event types; EventSink + SSE stream adapter; +7 tests (S26) |
| sera-plugins | ✅ IN PROGRESS | — | 48 | gRPC plugin registry, SDK, circuit breaker; public API re-exports + integration tests (S26) |

---

## 3. Per-Spec Gap Analysis

### SPEC-runtime ⚠️ 96% Complete

**Implemented:**
- TurnOutcome type (6 variants), ContextEngine trait, 15+ tools
- Tool executor, LLM client (multi-provider), session manager
- Compaction strategy framework, subagent management, delegation, handoff
- All 9 condensers fully implemented and tested (NoOp, RecentEvents, ConversationWindow, AmortizedForgetting, ObservationMasking, BrowserOutput, LlmSummarizing, LlmAttention, StructuredSummary)
- `ToolUseBehavior` discriminated union runtime enforcement (S26)

**Remaining Gaps:**
- `HarnessSupportContext` and `supports()` capability negotiation
- `ReactMode::PlanAndAct` planning phase not separated

**Files:** `rust/crates/sera-runtime/src/`

---

### SPEC-gateway ⚠️ 92% Complete

**Implemented:**
- AppServerTransport (Stdio, HTTP, WebSocket, gRPC), SQ/EQ envelope
- 35+ route handlers, Discord connector, lane queue (5 modes)
- Session persistence, transcript recording, circuit breaker, dedup

**Remaining Gaps (20 TODOs across 8 files):**
- LSP server routing not wired (`routes/lsp.rs`)
- Process status persistence (`services/process_manager.rs`)
- OIDC session mapping (`routes/oidc.rs`)
- Intercom manifest resolution (`routes/intercom.rs`)
- LLM proxy auth context extraction (`routes/llm_proxy.rs`)
- Pipeline executor spawning (`routes/pipelines.rs`)
- Change artifact population from gateway pipeline (`bin/sera.rs`)

**Files:** `rust/crates/sera-gateway/src/`

---

### SPEC-hooks ✅ 95% Complete

**Implemented:**
- `Hook` trait (async), `HookRegistry`, `ChainExecutor`
- `HookContext`, `HookResult`, `HookOutcome` types
- All `HookPoint` variants in sera-types
- **WASM adapter exists** (`wasm_adapter.rs`, feature-gated with wasmtime)
- `PermissionOverrides` in HookResult (S26)
- `HookCancellation` async cancellation (S26)
- `updated_input` transformation support (S26)

**Remaining Gaps:**
- WASM fuel metering and memory caps not configured
- Two-tier hook bus (Internal vs Plugin)

**Files:** `rust/crates/sera-hooks/src/`

---

### SPEC-memory ✅ 85% Complete (via sera-session)

**Implemented:**
- Four-tier ABC (Unconstrained, Token, SlidingWindow, Summarize)
- `MemoryBackend` trait via MemoryWrapper
- `MemoryEntry` with ephemeral/Wisp support, content-hash MemoryId

**Remaining Gaps:**
- No dedicated `sera-memory` crate (embedded in sera-session)
- RAG / embedding-based search not implemented
- PostgreSQL + Qdrant backend (enterprise) deferred
- `WorkflowMemoryManager` for Circle coordination

**Files:** `rust/crates/sera-session/src/memory_wrapper.rs`

---

### SPEC-workflow-engine ✅ 92% Complete

**Implemented:**
- Full workflow engine: types, registry, scheduling, dreaming config
- Atomic claim protocol with stale reaper
- Topological sort, SCC (Tarjan) cycle detection
- Termination detection, coordination with ConcurrencyScheduler
- Ready queue with dependency closure
- `AwaitType::Timer` gate + ready-queue integration (S26)

**Remaining Gaps:**
- `AwaitType` gates: GhRun, GhPr, Human, Mail, Change (Timer done)
- `WorkflowMemoryManager` coordinator-scoped summary
- `change_artifact_id` provenance tracking

**Files:** `rust/crates/sera-workflow/src/`

---

### SPEC-self-evolution ✅ 85% Complete

**Implemented (sera-meta, 2,196 LOC):**
- 3-tier evolution policy (Agent / Config / Code)
- Constitutional rule enforcement
- Approval matrix
- Artifact pipeline with full lifecycle
- Shadow session parallel validation

**Remaining Gaps:**
- Integration with gateway pipeline (`change_artifact: None` in sera.rs)
- Self-modification prevention guards
- `meta_scope` BlastRadius field fully wired in workflow

**Files:** `rust/crates/sera-meta/src/`

---

### SPEC-tools ✅ 100% Complete

All sandbox providers, SSRF validation, bash AST, kill switch, binary identity implemented.

### SPEC-identity-authz ✅ 98% Complete

JWT, OIDC, API keys, argon2, casbin RBAC adapter, capability tokens. RoleBasedAuthzProvider Tier-1.5 + ActionKind landed (S26). Minor gap: RBAC policy enforcement not fully integrated end-to-end.

### SPEC-observability ✅ 100% Complete

OTel triad version-pinned, AuditBackend, LaneFailureClass (15 variants), OCSF audit.

### SPEC-config ✅ 100% Complete

Figment, schema registry, manifest loader, env override, file watcher. `shadow_store.commit_overlay` bugfix landed (S26).

### SPEC-secrets ⚠️ 80% Complete

Env, Docker, File, Chained providers + enterprise scaffolds; 53 tests across 5 providers (S26). Vault, AWS SM, Azure KV, secret rotation still deferred.

### SPEC-deployment ⚠️ 50% Complete

Dockerfile + docker-compose exist. K8s manifests, multi-instance, BYOH deployment missing.

### SPEC-hitl-approval ✅ 80% Complete

Full approval workflow with escalation chains. Remaining: speculative execution during wait, timeout handling.

### SPEC-circles ⚠️ 40% Complete

Design types + coordination scaffold in sera-workflow. Full 7-policy implementation, blackboard, convergence incomplete.

### SPEC-interop ⚠️ 65% Complete

sera-mcp (70 tests — gating, rmcp_bridge, errors), sera-a2a (15 tests — Client, InProcRouter, Capabilities), sera-agui (17 tests — EventSink, SSE stream adapter) all substantively implemented in S26. Core protocol shapes complete; full end-to-end gateway integration pending.

### SPEC-plugins ⚠️ 50% Complete

Public API re-exports + integration tests landed (S26); 48 tests. gRPC registry, SDK, circuit breaker scaffolded. Full plugin lifecycle and hot-reload not yet wired.

---

## 4. Dependencies Graph

```
                    ┌─────────────────── sera-types (leaf) ─────────────────────┐
                    │                (no internal deps)                         │
                    └────────────┬────────────┬────────────┬───────┬────────────┘
                                │            │            │       │
             ┌──────────────────┘  ┌─────────┘  ┌────────┘       │
             │                     │             │                │
    sera-config              sera-db        sera-queue      sera-telemetry
    sera-errors              sera-auth      sera-events     sera-cache
    sera-secrets             sera-session                   sera-secrets
    sera-hooks               sera-workflow
    sera-tools               sera-models
    sera-hitl                sera-skills
    sera-meta ──► sera-events
                                     │
                    ┌────────────────┘
                    ▼
              sera-runtime ──► sera-hooks, sera-hitl, sera-config, sera-db
                    │
                    ▼
              sera-gateway ──► ALL ABOVE (aggregator hub)
                    │
              sera-tui ──► sera-types + reqwest
              sera-byoh-agent ──► sera-types + sera-config
              sera-testing ──► sera-types + sera-db + sera-queue + sera-tools

Phase 3 crates (IN PROGRESS):
  ├── sera-mcp ────────────── MCP server/client bridge (70 tests)
  ├── sera-a2a ────────────── A2A protocol adapter (15 tests)
  ├── sera-agui ───────────── AG-UI streaming (17 tests)
  └── sera-plugins ────────── gRPC plugin system (48 tests)
```

---

## 5. Next Steps (Prioritized)

### High Priority (P1) — Runtime Polish

1. **sera-gateway TODO cleanup** — Wire LSP routing, process management, artifact HTTP routes, pipeline spawning
2. **sera-runtime capability negotiation** — `HarnessSupportContext` and `supports()` + `ReactMode::PlanAndAct` separation
3. **Hooks two-tier bus** — Internal vs Plugin hook bus + WASM fuel metering

### Medium Priority (P2) — Domain Completion

1. **sera-secrets Vault/cloud providers** — Add Vault, AWS SM, Azure KV backends
2. **sera-errors adoption** — Integrate error codes across crates
3. **sera-auth RBAC wiring** — Complete casbin policy enforcement end-to-end
4. **Circles coordination** — Complete 7 coordination policies in sera-workflow
5. **sera-workflow AwaitType gates** — GhRun, GhPr, Human, Mail, Change (Timer done)

### Low Priority (P3) — Interop Completion

1. **sera-mcp** — Full end-to-end gateway integration (core protocol shapes done, 70 tests)
2. **sera-a2a** — Complete federation layer (Client + InProcRouter done, 15 tests)
3. **sera-agui** — Full stream wiring (EventSink + SSE done, 17 tests)
4. **sera-plugins** — Hot-reload + full plugin lifecycle (registry + SDK done, 48 tests)

### Deferred (P4)

1. **Enterprise auth** — OIDC/SCIM/AuthZen/SSF
2. **K8s deployment** — Manifests, multi-instance, leader election
3. **Redis cache** — sera-cache FredBackend
4. **LCM memory** — DAG-based lossless context management

---

## 6. Test Summary

| Crate | Tests | Status |
|-------|-------|--------|
| sera-gateway | 391 | ✅ PASS |
| sera-types | 334 | ✅ PASS |
| sera-runtime | 256 | ✅ PASS |
| sera-skills | 190 | ✅ PASS |
| sera-tools | 145 | ✅ PASS |
| sera-workflow | 110 | ✅ PASS |
| sera-telemetry | 91 | ✅ PASS |
| sera-config | 81 | ✅ PASS |
| sera-db | 79 | ✅ PASS |
| sera-session | 78 | ✅ PASS |
| sera-models | 75 | ✅ PASS |
| sera-meta | 72 | ✅ PASS |
| sera-mcp | 70 | ✅ PASS |
| sera-tui | 67 | ✅ PASS |
| sera-auth | 53 | ✅ PASS |
| sera-secrets | 53 | ✅ PASS |
| sera-byoh-agent | 52 | ✅ PASS |
| sera-plugins | 48 | ✅ PASS |
| sera-testing | 37 | ✅ PASS |
| sera-hooks | 37 | ✅ PASS |
| sera-events | 34 | ✅ PASS |
| sera-queue | 23 | ✅ PASS |
| sera-cache | 22 | ✅ PASS |
| sera-hitl | 20 | ✅ PASS |
| sera-agui | 17 | ✅ PASS |
| sera-a2a | 15 | ✅ PASS |
| sera-errors | 5 | ✅ PASS |
| **TOTAL** | **2,455** | **✅ ALL PASS** |

---

## 7. Change Log

| Date | Session | Changes |
|------|---------|---------|
| 2026-04-15 | S14 | Initial tracker creation |
| 2026-04-16 | S15b | Fresh assessment: corrected crate count (19→23), LOC (29K→64.6K), tests (500→1,196); updated sera-models/skills/meta/hitl/hooks/events from NOT STARTED/PARTIAL to COMPLETE; recalculated all phase percentages; corrected Phase 2 description |
| 2026-04-16 | S21 | Code audit: removed false "3 condenser stubs" claim (all 9 implemented); reconciled test counts per crate from #[test] grep; fixed clippy workspace-wide (17 fixes across 10 files); SPEC-runtime bumped 90%→93% |
| 2026-04-17 | S25 | Ultrawork marathon: 16 beads closed, ~95 new tests; Phase 2 bumped 85%→95%; gateway startup validation, runtime fixes, builder/querybuilder patterns, NDJSON protocol alignment, HybridScorer (586 LOC), 56% dead code reduction; 39 gateway stubs classified; ArtifactPipeline integrated; follow-ups filed for HTTP routes + HookContext threading |
| 2026-04-17 | S26 | Ultrawork marathon: ~20 beads closed, ~366 new tests across 20 crates (1,188→2,455 incl. tokio::test recount); Phase 1 90%→95%, Phase 2 95%→97%, Phase 3 60%→75%; key features: ToolUseBehavior runtime enforcement, PermissionOverrides+HookCancellation+updated_input in hooks, Timer gate (AwaitType::Timer), RoleBasedAuthzProvider Tier-1.5+ActionKind, commit_overlay bugfix (SPEC-config→100%), llm_proxy JWT impersonation fix, BYOH build_* seam extraction, contracts.rs golden YAML harness; corrected sera-models stale 0→75 tests, sera-events 12→34; all SPEC-interop crates promoted from SCAFFOLDED to IN PROGRESS |

---

*Updated 2026-04-17 by Session 26 ultrawork marathon*

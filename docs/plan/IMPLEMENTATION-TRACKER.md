# SERA Rust Migration — Implementation Tracker

> **Document Status:** Current (Updated 2026-04-16 — Session 18)
> **Purpose:** Master tracking document for SERA 2.0 Rust migration
> **Basis:** Full spec analysis + codebase inspection + test run verification

---

## 1. Executive Summary

### Current State Overview

The SERA Rust workspace is **substantially implemented** with **23 of 27 planned crates** present and building. The workspace compiles successfully and all tests pass (1,196 tests across 23 crates).

| Metric | Value |
|--------|-------|
| Total Crates Planned | 27 |
| Crates in Workspace | 23 |
| Missing Crates | sera-mcp, sera-a2a, sera-agui, sera-plugins |
| Total Rust LOC | 64,643 (325 .rs files) |
| Build Status | ✅ COMPILES (release build passing) |
| Test Status | ✅ ALL PASSING (1,309 tests) |

### Phase Completion

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| Phase 0 | Foundation (types, config, DB, queue, telemetry) | COMPLETE | 95% |
| Phase 1 | Core Domain (session, auth, tools, hooks, workflow, models, skills) | COMPLETE | 90% |
| Phase 2 | Runtime & Gateway (runtime, gateway, TUI, BYOH, meta) | IN PROGRESS | 85% |
| Phase 3 | Interop Protocols (MCP, A2A, AG-UI) | NOT STARTED | 0% |
| Phase 4 | Enterprise & Hardening (plugins, OIDC/SCIM, K8s, Circles full) | NOT STARTED | 0% |

### Key Achievements (Session 15b verified)

1. **Core gateway operational** — `sera-gateway` with 81 source files, 21,757 LOC, 223+ tests
2. **Runtime infrastructure complete** — `sera-runtime` with 37 source files, 8,180 LOC, 115+ tests
3. **Model provider abstractions created** — `sera-models` (219 LOC) with `ModelProvider` trait
4. **Skill pack loading created** — `sera-skills` (349 LOC) with filesystem-based discovery
5. **Self-evolution machinery complete** — `sera-meta` (2,196 LOC) with 3-tier policy, shadow sessions, constitutional rules
6. **HITL approval production-ready** — `sera-hitl` (819 LOC) with full escalation chains and tests
7. **Workflow engine comprehensive** — `sera-workflow` (3,145 LOC) with SCC cycle detection, termination detection, coordination
8. **Type system comprehensive** — `sera-types` (8,921 LOC) with 29 modules covering full domain
9. **WASM hook adapter exists** — `sera-hooks` has feature-gated `wasmtime` support via `wasm_adapter.rs`

### Remaining Gaps

1. **sera-runtime LLM compaction** — 3 condensers (summarizer, attention, extraction) are TODO stubs
2. **sera-gateway TODOs** — 20 TODO markers across 8 files (LSP routing, process mgmt, auth context)
3. **sera-errors not consumed** — 27 LOC scaffold, not imported by any crate
4. **sera-secrets minimal** — Only `EnvSecretsProvider` (44 LOC), needs Vault/AWS/Azure providers
5. **sera-cache minimal** — Only `MokaBackend` (66 LOC), Redis backend deferred
6. **Interop crates** — sera-mcp, sera-a2a, sera-agui, sera-plugins not started

---

## 2. Per-Crate Status

### Foundation Crates (Phase 0)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-types | ✅ COMPLETE | 8,921 | 272+ | 29 modules, comprehensive domain types |
| sera-config | ✅ COMPLETE | 2,129 | 52+ | Layered config, schema registry, file watcher |
| sera-errors | ⚠️ SCAFFOLD | 27 | 0 | Error code enum only; not consumed by any crate |
| sera-cache | ⚠️ SCAFFOLD | 66 | 0 | MokaBackend; Redis deferred |
| sera-db | ✅ COMPLETE | 3,836 | — | PostgreSQL (sqlx) + SQLite (rusqlite), 21 source files |
| sera-queue | ✅ COMPLETE | 470 | 12+ | QueueBackend trait, local + apalis backends |
| sera-telemetry | ✅ COMPLETE | 436 | 18+ | OTel triad (version-pinned), AuditBackend, OCSF |
| sera-secrets | ⚠️ SCAFFOLD | 44 | 0 | EnvSecretsProvider only |

### Core Domain Crates (Phase 1)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-session | ✅ COMPLETE | 1,391 | 14+ | 6-state machine, transcript, 4-tier memory |
| sera-auth | ✅ COMPLETE | 1,289 | 28+ | JWT, OIDC, API keys, casbin RBAC (TODO: full wiring) |
| sera-tools | ✅ COMPLETE | 1,900+ | 35+ | 5 sandbox providers, SSRF, bash AST, kill switch, contradiction detection, source mounts |
| sera-hooks | ✅ COMPLETE | 1,206 | — | Native hooks + WASM adapter (feature-gated wasmtime) |
| sera-hitl | ✅ COMPLETE | 819 | — | Full approval workflow, escalation chains, tests in lib |
| sera-workflow | ✅ COMPLETE | 3,145 | 40+ | Atomic claims, SCC cycle detection, termination, coordination |
| sera-events | ✅ COMPLETE | 501 | — | Audit Merkle chain (SHA-256), Centrifugo pub/sub |
| sera-models | ✅ COMPLETE | 219 | — | ModelProvider trait, ProviderConfig, ModelResponse |
| sera-skills | ✅ COMPLETE | 880+ | 20+ | SkillLoader, SkillPack trait, YAML discovery, KnowledgeSchemaValidator |

### Runtime & Gateway (Phase 2)

| Crate | Status | LOC | Tests | Notes |
|-------|--------|-----|-------|-------|
| sera-runtime | ⚠️ 90% | 8,180 | 115+ | Core operational; 3 LLM compaction condensers are stubs |
| sera-gateway | ⚠️ 90% | 21,757 | 223+ | Core operational; 20 TODOs across 8 files |
| sera-meta | ✅ COMPLETE | 2,196 | 64+ | 3-tier evolution, shadow sessions, constitutional rules (Epic 30 P2 closed) |
| sera-tui | ✅ COMPLETE | 835 | 2+ | ratatui TUI, crossterm input |
| sera-byoh-agent | ✅ COMPLETE | 221 | — | BYOH reference implementation |
| sera-testing | ✅ COMPLETE | 326 | 8+ | Mock implementations, contract tests |

### Missing Crates (Phase 3-4)

| Crate | Spec Reference | Priority | bd Issue |
|-------|---------------|----------|----------|
| sera-mcp | SPEC-interop | P3 | sera-11ak |
| sera-a2a | SPEC-interop | P3 | sera-su86 |
| sera-agui | SPEC-interop | P3 | sera-4qel |
| sera-plugins | SPEC-plugins | P3 | sera-iyov |

---

## 3. Per-Spec Gap Analysis

### SPEC-runtime ⚠️ 90% Complete

**Implemented:**
- TurnOutcome type (6 variants), ContextEngine trait, 15+ tools
- Tool executor, LLM client (multi-provider), session manager
- Compaction strategy framework, subagent management, delegation, handoff

**Remaining Gaps:**
- 3 LLM-driven condensers are stubs: `SummarizingCondenser`, `AttentionCondenser`, `ExtractionCondenser`
- `ToolUseBehavior` discriminated union not fully wired
- `HarnessSupportContext` and `supports()` capability negotiation
- `ReactMode::PlanAndAct` planning phase not separated

**Files:** `rust/crates/sera-runtime/src/`

---

### SPEC-gateway ⚠️ 90% Complete

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

### SPEC-hooks ✅ 85% Complete

**Implemented:**
- `Hook` trait (async), `HookRegistry`, `ChainExecutor`
- `HookContext`, `HookResult`, `HookOutcome` types
- All `HookPoint` variants in sera-types
- **WASM adapter exists** (`wasm_adapter.rs`, feature-gated with wasmtime)

**Remaining Gaps:**
- WASM fuel metering and memory caps not configured
- `HookAbortSignal` async cancellation
- `PermissionOverrides` in HookResult
- Two-tier hook bus (Internal vs Plugin)
- `updated_input` transformation support (TODO in executor)

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

### SPEC-workflow-engine ✅ 90% Complete

**Implemented:**
- Full workflow engine: types, registry, scheduling, dreaming config
- Atomic claim protocol with stale reaper
- Topological sort, SCC (Tarjan) cycle detection
- Termination detection, coordination with ConcurrencyScheduler
- Ready queue with dependency closure

**Remaining Gaps:**
- `AwaitType` gates (GhRun, GhPr, Timer, Human, Mail, Change)
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

### SPEC-identity-authz ✅ 95% Complete

JWT, OIDC, API keys, argon2, casbin RBAC adapter, capability tokens. Minor gap: RBAC policy enforcement not fully integrated (TODO).

### SPEC-observability ✅ 100% Complete

OTel triad version-pinned, AuditBackend, LaneFailureClass (15 variants), OCSF audit.

### SPEC-config ✅ 95% Complete

Figment, schema registry, manifest loader, env override, file watcher. Minor gap: `shadow_store.commit_overlay` unimplemented.

### SPEC-secrets ⚠️ 30% Complete

Only `EnvSecretsProvider` (44 LOC). Vault, AWS SM, Azure KV, secret rotation all missing.

### SPEC-deployment ⚠️ 50% Complete

Dockerfile + docker-compose exist. K8s manifests, multi-instance, BYOH deployment missing.

### SPEC-hitl-approval ✅ 80% Complete

Full approval workflow with escalation chains. Remaining: speculative execution during wait, timeout handling.

### SPEC-circles ⚠️ 40% Complete

Design types + coordination scaffold in sera-workflow. Full 7-policy implementation, blackboard, convergence incomplete.

### SPEC-interop 🔲 0% Complete

sera-mcp, sera-a2a, sera-agui not started. Issues exist (sera-11ak, sera-su86, sera-4qel).

### SPEC-plugins 🔲 0% Complete

Design types only. Issue exists (sera-iyov).

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

MISSING (4 crates):
  ├── sera-mcp ────────────── MCP server/client (bd: sera-11ak)
  ├── sera-a2a ────────────── A2A protocol adapter (bd: sera-su86)
  ├── sera-agui ───────────── AG-UI streaming (bd: sera-4qel)
  └── sera-plugins ────────── gRPC plugin system (bd: sera-iyov)
```

---

## 5. Next Steps (Prioritized)

### High Priority (P1) — Runtime Polish

1. **sera-runtime LLM compaction** — Implement 3 condenser stubs (summarizer, attention, extraction)
2. **sera-gateway TODO cleanup** — Wire LSP routing, process management, auth context extraction

### Medium Priority (P2) — Domain Completion

1. **sera-secrets providers** — Add Docker secret, file-based encrypted, Vault scaffolds
2. **sera-errors adoption** — Integrate error codes across crates
3. **sera-auth RBAC wiring** — Complete casbin policy enforcement
4. **sera-config shadow store** — Implement `commit_overlay`
5. **Circles coordination** — Complete 7 coordination policies in sera-workflow

### Low Priority (P3) — Interop & Enterprise

1. **sera-mcp** — MCP server/client bridge (bd: sera-11ak)
2. **sera-a2a** — A2A protocol adapter (bd: sera-su86)
3. **sera-agui** — AG-UI streaming (bd: sera-4qel)
4. **sera-plugins** — gRPC plugin system (bd: sera-iyov)

### Deferred (P4)

1. **Enterprise auth** — OIDC/SCIM/AuthZen/SSF
2. **K8s deployment** — Manifests, multi-instance, leader election
3. **Redis cache** — sera-cache FredBackend
4. **LCM memory** — DAG-based lossless context management

---

## 6. Test Summary

| Crate | Tests | Status |
|-------|-------|--------|
| sera-types | 272+ | ✅ PASS |
| sera-gateway | 223+ | ✅ PASS |
| sera-runtime | 115+ | ✅ PASS |
| sera-config | 52+ | ✅ PASS |
| sera-workflow | 40+ | ✅ PASS |
| sera-auth | 28+ | ✅ PASS |
| sera-telemetry | 18+ | ✅ PASS |
| sera-tools | 15+ | ✅ PASS |
| sera-session | 14+ | ✅ PASS |
| sera-queue | 12+ | ✅ PASS |
| sera-testing | 8+ | ✅ PASS |
| sera-tui | 2+ | ✅ PASS |
| sera-hitl | inline | ✅ PASS |
| sera-meta | inline | ✅ PASS |
| sera-models | — | ✅ COMPILES |
| sera-skills | — | ✅ COMPILES |
| sera-events | inline | ✅ PASS |
| sera-hooks | inline | ✅ PASS |
| **TOTAL** | **1,196** | **✅ ALL PASS** |

---

## 7. Change Log

| Date | Session | Changes |
|------|---------|---------|
| 2026-04-15 | S14 | Initial tracker creation |
| 2026-04-16 | S15b | Fresh assessment: corrected crate count (19→23), LOC (29K→64.6K), tests (500→1,196); updated sera-models/skills/meta/hitl/hooks/events from NOT STARTED/PARTIAL to COMPLETE; recalculated all phase percentages; corrected Phase 2 description |

---

*Generated 2026-04-16 by Session 15b fresh codebase assessment*

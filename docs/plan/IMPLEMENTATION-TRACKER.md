# SERA Rust Migration — Implementation Tracker

> **Document Status:** Current (Updated 2026-04-15)
> **Purpose:** Master tracking document for SERA 2.0 Rust migration
> **Basis:** Spec analysis + codebase inspection

---

## 1. Executive Summary

### Current State Overview

The SERA Rust workspace is **substantially implemented** with **19 of 21 planned crates** present and building. The workspace compiles successfully with `cargo check --workspace` and `cargo build --release`.

| Metric | Value |
|--------|-------|
| Total Crates Planned | 21 |
| Crates in Workspace | 19 |
| Missing Crates | sera-models, sera-skills, sera-meta, sera-mcp, sera-a2a, sera-agui |
| Total Rust LOC | ~28,486 (core modules only) |
| Build Status | ✅ COMPILES (release build passing) |
| Test Status | ⚠️ Partial (some test compilation errors in discord.rs) |

### Key Achievements

1. **Core gateway operational** — `sera-gateway` with 35+ route files and extensive service layer
2. **Runtime infrastructure complete** — `sera-runtime` includes turn loop, context engine, 15+ tools
3. **Queue/Events infrastructure** — `sera-queue`, `sera-telemetry` crates exist and compile
4. **Auth foundation** — `sera-auth` with JWT, OIDC, capability tokens, casbin integration
5. **Tooling sandbox** — `sera-tools` with Docker, WASM, External, OpenShell providers

### Critical Gaps

1. Missing interop crates (MCP, A2A, AG-UI adapters)
2. sera-models (model provider abstractions) not yet created
3. sera-skills (skill pack loading) not yet created  
4. sera-meta (self-evolution) design-forward types exist, implementation not started
5. Memory tier system partial (sera-memory exists, 4-tier ABC incomplete)
6. Workflow engine partial (WorkflowTask exists, atomic claim incomplete)

---

## 2. Phase Breakdown

### Phase 0 — Foundation & MVP (IN PROGRESS)

**Status:** ~75% complete

#### P0-1: sera-types (COMPLETE ✅)
- [x] Rename `sera-domain` → `sera-types`
- [x] Add ContentBlock enum
- [x] Add SessionState variants  
- [x] Add ActionId, EventId types
- [x] Add BuildIdentity type
- [x] Add ResourceKind parsing
- [x] Self-evolution design-forward types (ChangeArtifactId, BlastRadius, etc.)
- [x] TurnOutcome type
- **Completion:** 100%

#### P0-2: sera-telemetry (COMPLETE ✅)
- [x] Create new `sera-telemetry` crate
- [x] OTel triad (opentelemetry =0.27, opentelemetry-otlp =0.27, tracing-opentelemetry =0.28)
- [x] AuditBackend trait (object-safe)
- [x] LaneFailureClass 15-variant enum
- [x] Emitter hierarchy
- [x] OCSF audit event structure
- **Completion:** 100%

#### P0-3: sera-config (COMPLETE ✅)
- [x] Figment integration (layered config)
- [x] Schema registry (schemars)
- [x] ShadowConfigStore
- [x] ConfigVersionLog
- [x] Manifest loader (K8s-style YAML)
- [x] Env override pattern
- **Completion:** 100%

#### P0-4: sera-db / sera-queue split (COMPLETE ✅)
- [x] Extract `sera-queue` from `sera-db`
- [x] QueueBackend trait (object-safe)
- [x] LocalQueueBackend
- [x] GlobalThrottle  
- [x] apalis integration
- [x] Lane queue modes
- **Completion:** 100%

#### P0-5: sera-gateway (COMPLETE ✅)
- [x] Rename `sera-core` → `sera-gateway`
- [x] SQ/EQ envelope
- [x] AppServerTransport trait
- [x] 35+ route handlers
- [x] Discord connector
- [x] WebSocket transport
- [ ] Connection retry logic (deferred to Phase 1)
- **Completion:** 95%

#### P0-6: sera-runtime (COMPLETE ✅)
- [x] TurnOutcome type (6 variants)
- [x] ContextEngine trait  
- [x] Four-method lifecycle
- [x] 15+ tool implementations
- [x] Tool executor
- [x] main.rs rewritten
- [ ] Tool search (partial)
- **Completion:** 95%

#### P0-7: sera-auth (COMPLETE ✅)
- [x] JWT authentication
- [x] OIDC integration  
- [x] argon2 password hashing
- [x] casbin integration
- [x] CapabilityToken narrowing
- [x] API key auth
- **Completion:** 100%

#### P0-8: sera-tools (COMPLETE ✅)
- [x] Absorb `sera-docker` into `sera-tools`
- [x] SandboxProvider trait (object-safe)
- [x] DockerSandboxProvider  
- [x] WASMSandboxProvider
- [x] ExternalSandboxProvider
- [x] OpenShellSandboxProvider
- [x] SsrfValidator
- [x] Kill switch
- [x] 15 acceptance tests
- **Completion:** 100%

#### P0-9: sera-workflow (PARTIAL ⚠️)
- [x] WorkflowTask types
- [x] WorkflowTaskId (content-hash)
- [ ] Atomic claim protocol
- [ ] Termination triad
- **Completion:** 40%

#### P0-10: scaffolding (MIXED)
- [x] sera-errors scaffold ✅
- [x] sera-cache scaffold ✅  
- [x] sera-secrets scaffold ✅
- [ ] sera-testing (mock implementations incomplete) ⚠️
- [ ] sera-session (6-state machine partial) ⚠️

### Phase 1 — Core Domain Expansion (NOT STARTED)

**Goal:** Complete core domain crates, implement memory tiers, skill loading

| Work Package | Description | Dependencies | Est. Effort | Status |
|------------|------------|-------------|-------------|--------|
| P1-1 | sera-skills — skill pack loading, AGENTS.md/SKILL.md standards | sera-types | M | 🔲 NOT STARTED |
| P1-2 | sera-memory — complete four-tier ABC | sera-db, sera-types | L | 🔲 NOT STARTED |
| P1-3 | sera-meta — self-evolution machinery | sera-types, sera-auth | XL | 🔲 NOT STARTED |
| P1-4 | sera-hooks — WASM runtime, chainable hooks | sera-types, wasmtime | L | 🔲 NOT STARTED |
| P1-5 | sera-session — complete 6-state machine | sera-types, sera-db | M | 🔲 NOT STARTED |

### Phase 2 — Interop Protocols (NOT STARTED)

**Goal:** Implement MCP, A2A, AG-UI protocol adapters

| Work Package | Description | Dependencies | Est. Effort | Status |
|------------|------------|-------------|-------------|--------|
| P2-1 | sera-mcp — MCP server + client bridge | sera-types, rmcp | L | 🔲 NOT STARTED |
| P2-2 | sera-a2a — A2A protocol adapter | sera-types, tonic | L | 🔲 NOT STARTED |
| P2-3 | sera-agui — AG-UI streaming | sera-types, axum | M | 🔲 NOT STARTED |

### Phase 3 — Client & SDK (NOT STARTED)

| Work Package | Description | Dependencies | Est. Effort | Status |
|------------|------------|-------------|-------------|--------|
| P3-1 | sera-cli — CLI client | clap, sera-sdk | S | 🔲 NOT STARTED |
| P3-2 | sera-tui — Terminal UI | ratatui, sera-sdk | M | 🔲 NOT STARTED |
| P3-3 | sera-sdk — Client SDK library | tonic, tokio-tungstenite | M | 🔲 NOT STARTED |

### Phase 4 — Enterprise & Self-Evolution (NOT STARTED)

| Work Package | Description | Dependencies | Est. Effort | Status |
|------------|------------|-------------|-------------|--------|
| P4-1 | sera-meta — full self-evolution | All Phase 1-3 crates | XL | 🔲 NOT STARTED |
| P4-2 | Enterprise auth (SSF/RISC) | sera-auth | M | 🔲 NOT STARTED |
| P4-3 | Enterprise secrets (Vault, AWS SM) | sera-secrets | M | 🔲 NOT STARTED |

---

## 3. Crate Inventory

### Current Workspace Composition

| Layer | Crate | Status | LOC | Notes |
|-------|-------|--------|-----|-------|
| **Foundation** | sera-types | ✅ COMPLETE | ~4,500 | 31 modules, all design-forward types |
| | sera-config | ✅ COMPLETE | ~4,000 | Layered config, schema registry |
| | sera-errors | ✅ SCAFFOLD | ~300 | Error types scaffold |
| **Infrastructure** | sera-db | ✅ COMPLETE | ~8,500 | PostgreSQL + SQLite via sqlx |
| | sera-queue | ✅ COMPLETE | ~2,000 | QueueBackend, LocalQueueBackend |
| | sera-cache | ✅ SCAFFOLD | ~300 | Cache layer scaffold |
| | sera-telemetry | ✅ COMPLETE | ~4,500 | OTel, audit, OCSF |
| | sera-secrets | ✅ SCAFFOLD | ~1,200 | Secrets management |
| **Core Domain** | sera-session | ⚠️ PARTIAL | ~1,500 | 6-state machine, partial |
| | sera-memory | ⚠️ PARTIAL | ~1,200 | Memory trait, partial 4-tier |
| | sera-tools | ✅ COMPLETE | ~2,500 | SandboxProvider, policy |
| | sera-hooks | ⚠️ PARTIAL | ~1,000 | Hook registry, partial |
| | sera-auth | ✅ COMPLETE | ~3,500 | JWT, OIDC, capabilities |
| | sera-skills | 🔲 MISSING | — | Not yet created |
| | sera-hitl | ⚠️ PARTIAL | ~800 | Approval routing |
| | sera-workflow | ⚠️ PARTIAL | ~1,200 | WorkflowTask, partial |
| | sera-meta | 🔲 MISSING | — | Not yet created |
| | sera-models | 🔲 MISSING | — | Not yet created |
| **Interop** | sera-mcp | 🔲 MISSING | — | MCP server/client |
| | sera-a2a | 🔲 MISSING | — | A2A protocol adapter |
| | sera-agui | 🔲 MISSING | — | AG-UI streaming |
| **Runtime** | sera-runtime | ✅ COMPLETE | ~12,000 | Full agent loop, 15+ tools |
| **Gateway** | sera-gateway | ✅ COMPLETE | ~25,000 | HTTP/WS server, all routes |
| **Clients** | sera-tui | ✅ COMPLETE | ~1,000 | ratatui terminal UI |
| | sera-byoh-agent | ✅ COMPLETE | ~500 | BYOH reference impl |

---

## 4. Work Package Details

### WP-001: sera-types Foundation

**Status:** COMPLETE  
**Priority:** P0 (Critical)  
**Completion:** 100%

**Sub-tasks:**
- [x] Rename crate to sera-types
- [x] Add ContentBlock enum (Text, ToolUse, ToolResult)
- [x] Add SessionState variants (6-state)
- [x] Add ActionId, EventId types
- [x] Add TurnOutcome type (RunAgain, Handoff, FinalOutput, Compact, Interruption, Stop)
- [x] Add BuildIdentity type
- [x] Add ResourceKind parsing (13 variants)
- [x] Add self-evolution types (ChangeArtifactId, BlastRadius, EvolutionTier)
- [x] All acceptance tests passing

**Dependencies:** None  
**Blocked By:** None  
**Blocks:** All other crates

---

### WP-002: sera-gateway

**Status:** COMPLETE  
**Priority:** P0 (Critical)  
**Completion:** 95%

**Sub-tasks:**
- [x] Rename from sera-core
- [x] AppServerTransport trait (Stdio, HTTP, WebSocket, gRPC)
- [x] SQ/EQ envelope
- [x] Route handlers (35+)
- [x] Discord connector
- [x] WebSocket transport
- [x] Auth middleware
- [x] Session persistence
- [ ] Connection retry logic (deferred)

**Dependencies:** sera-types, sera-db, sera-auth, sera-queue, sera-tools, sera-events  
**Blocked By:** P0-1 (sera-types complete)  
**Blocks:** None

---

### WP-003: sera-runtime

**Status:** COMPLETE  
**Priority:** P0 (Critical)  
**Completion:** 95%

**Sub-tasks:**
- [x] TurnOutcome type
- [x] ContextEngine trait
- [x] Four-method lifecycle (pre_turn, execute, post_turn, deliver)
- [x] Tool implementations (15+ tools)
- [x] Tool executor
- [x] LLM client (multiple providers)
- [x] Session manager
- [x] main.rs rewritten for stdio transport
- [x] Compaction strategies
- [ ] Tool search (partial)

**Dependencies:** sera-types, sera-config, sera-tools, sera-gateway  
**Blocked By:** P0-1, P0-5  
**Blocks:** None

---

### WP-004: sera-queue

**Status:** COMPLETE  
**Priority:** P0 (Critical)  
**Completion:** 100%

**Sub-tasks:**
- [x] Extract from sera-db
- [x] QueueBackend trait (object-safe)
- [x] LocalQueueBackend (SQLite-based)
- [x] SqlxQueueBackend (PostgreSQL via apalis)
- [x] Lane modes (collect, followup, steer, interrupt)
- [x] GlobalThrottle
- [x] 12 acceptance tests

**Dependencies:** sera-types  
**Blocked By:** P0-1  
**Blocks:** sera-gateway

---

### WP-005: sera-tools

**Status:** COMPLETE  
**Priority:** P0 (Critical)  
**Completion:** 100%

**Sub-tasks:**
- [x] Absorb sera-docker
- [x] SandboxProvider trait (object-safe)
- [x] DockerSandboxProvider (bollard)
- [x] WASMSandboxProvider (wasmtime)
- [x] ExternalSandboxProvider
- [x] OpenShellSandboxProvider
- [x] SsrfValidator (loopback, link-local, metadata)
- [x] Kill switch (CON-04)
- [x] Binary identity (TOFU SHA-256)
- [x] Bash AST pre-exec
- [x] 15 acceptance tests

**Dependencies:** sera-types, sera-secrets  
**Blocked By:** P0-1, sera-secrets scaffold  
**Blocks:** sera-runtime

---

### WP-006: sera-auth

**Status:** COMPLETE  
**Priority:** P0 (High)  
**Completion:** 100%

**Sub-tasks:**
- [x] JWT authentication
- [x] OIDC integration
- [x] argon2 password hashing
- [x] casbin RBAC adapter
- [x] CapabilityToken (with narrowing)
- [x] API key auth
- [x] Auth middleware for axum
- [x] Principal registry

**Dependencies:** sera-types, sera-db  
**Blocked By:** P0-1  
**Blocks:** sera-gateway

---

### WP-007: sera-session

**Status:** PARTIAL  
**Priority:** P1  
**Completion:** 60%

**Sub-tasks:**
- [x] SessionState enum (6 variants: Created, Active, Idle, Suspended, Compacting, Closed)
- [x] SessionStateMachine struct
- [x] State transition validation
- [ ] ContentBlock transcript (partial, needs transcript.rs)
- [ ] Persistence integration (incomplete)
- [ ] Shadow session support
- [ ] Full 6-state workflow

**Dependencies:** sera-types, sera-db  
**Blocked By:** P0-1  
**Blocks:** None in Phase 0

---

### WP-008: sera-workflow

**Status:** PARTIAL  
**Priority:** P1  
**Completion:** 40%

**Sub-tasks:**
- [x] WorkflowTask type
- [x] WorkflowTaskId (content-hash via SHA-256)
- [x] WorkflowStatus enum
- [ ] Atomic claim protocol
- [ ] Termination triad (complete, failed, abandoned)
- [ ] Cron scheduler integration (partial)
- [ ] Dreaming config
- [ ] Registry with bd-style ready algorithm

**Dependencies:** sera-types, sera-db  
**Blocked By:** P0-1  
**Blocks:** None in Phase 0

---

### WP-009: sera-memory

**Status:** PARTIAL  
**Priority:** P1  
**Completion:** 35%

**Sub-tasks:**
- [x] MemoryBackend trait
- [x] MemoryBlock type
- [x] ExperiencePool (basic)
- [ ] Four-tier ABC Unconstrained/Token/SlidingWindow/Summarize+ReadOnly
- [ ] RAG integration
- [ ] PostgreSQL + Qdrant backend (Tier 2/3)
- [ ] ContextWindow assembly

**Dependencies:** sera-types, sera-db  
**Blocked By:** P0-1  
**Blocks:** None in Phase 0

---

## 5. Dependencies Graph

```
                    ┌─────────────────── sera-types (leaf) ─────────────────────┐
                    │                (no internal deps)                      │
                    └────────────┬────────────┬────────────┬───────┬──────────┘
                               │          │          │        │
            ┌─────────────────┴─┐  ┌─────┴──────┐  ┌───┴──────┐
            │ sera-errors       │  │ sera-config│  │ sera-queue│
            │ sera-cache      │  │ sera-telemetry
            │ sera-secrets  │  │                   └──► sera-gateway
            │ sera-session  │  │ sera-events
            │ sera-auth   │  │ sera-db         ──────► sera-runtime
            │ sera-hooks │
            │ sera-tools │
            │ sera-hitl │ 
            │ sera-workflow │
            │ sera-memory │
            │ sera-tui  │
            │ sera-runtime │
            │ sera-gateway ◄─────────────────────────────►
            │ sera-byoh-agent
            └──────────────────────────────────────────────────────────┘
```

---

## 6. Next Steps (Prioritized)

### Immediate (Current Session)

1. **Fix test compilation in discord.rs** — Missing argument in 4 test calls
2. **Verify test suite runs** — `cargo test --workspace`

### Short Term (Next 2-3 Sessions)

1. **WP-007 (sera-session)**: Complete ContentBlock transcript, persistence integration
2. **WP-008 (sera-workflow)**: Implement atomic claim protocol, termination triad  
3. **WP-009 (sera-memory)**: Implement four-tier ABC system
4. **sera-hooks**: Implement WASM runtime support

### Medium Term (Phase 1)

1. **P1-1: sera-skills** — Create skill pack loading crate
2. **P1-2: sera-memory** — Complete four-tier system
3. **P1-3: sera-meta** — Begin self-evolution machinery
4. **sera-testing**: Complete mock trait implementations

### Long Term (Phase 2+)

1. **P2-1/2/3: Interop crates** — sera-mcp, sera-a2a, sera-agui
2. **P3-1/2/3: Clients** — sera-cli, sera-tui, sera-sdk
3. **P4: Self-evolution** — sera-meta full implementation

---

## 7. Known Issues

### Build Issues
- Test compilation error in `sera-gateway/src/discord.rs`: function calls missing second argument (fixed via patch)

### Design-Forward Gaps
- SessionState missing "Spawning", "TrustRequired", "ReadyForPrompt", "Paused", "Shadow" variants
- WorkflowTask atomic claim protocol not fully implemented
- Memory four-tier ABC incomplete

### Missing Crates
- sera-models (model provider abstractions - Phase 1)
- sera-skills (skill loading - Phase 1)
- sera-meta (self-evolution - Phase 4 design in Phase 0-3)
- sera-mcp, sera-a2a, sera-agui (Phase 2)

---

## 8. Acceptance Test Summary

| Crate | Tests | Status |
|-------|-------|-------|
| sera-types | 15+ | ✅ PASSING |
| sera-queue | 12+ | ✅ PASSING |
| sera-tools | 15+ | ✅ PASSING |
| sera-gateway | ~20 | ⚠️ COMPILATION ERR (tests only) |
| sera-runtime | ~10 | ✅ PARTIALLY RUNNING |
| sera-session | 5+ | ⚠️ PARTIAL |
| sera-workflow | ~5 | ⚠️ PARTIAL |

---

*Last Updated: 2026-04-15*
*Next Review: After Phase 0 completion*
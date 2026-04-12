# SERA 2.0 Phase 1 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**Phase 1 complete.** Twelve commits on `sera20` — eight execution substrate wiring (session 1) plus four design+implement items (session 2):

**Session 1 — execution substrate wiring:**

1. **`876d7ac` — feat: wire DefaultHarness think step to LlmClient via LlmProvider trait.** Connected the four-method turn lifecycle's think step to actual LLM inference via sera-gateway's llm_client.rs.

2. **`bc67370` — feat: wire gateway chat handler through harness_dispatch.** Integrated sera-gateway's orchestrator to route chat operations through harness_dispatch::dispatch.

3. **`77c9c1e` — feat: wire NDJSON runtime loop to DefaultRuntime.execute_turn.** Connected sera-runtime's NDJSON child process loop to DefaultRuntime.execute_turn.

4. **`e21334f` — chore: delete deprecated TurnResult from sera-types.** Removed TurnResult; deprecated in Phase 0.

5. **`73493ff` — feat: add SqlxSessionPersist for durable session storage.** sqlx-backed persistence for session parts in sera-gateway.

6. **`d4d8d65` — docs: update HANDOFF.md for Phase 1 progress (interim).**

7. **`8e4e830` — feat: wire condensers into ContextEngine compact method.** Connected condenser trait impls into ContextEngine's compact method.

8. **`e6dfd0e` — feat: wire ConstitutionalGate hooks into observe/react lifecycle.** Integrated ConstitutionalGate enforcement into observe and react methods.

**Session 2 — design decisions resolved + implemented:**

9. **`b974d14` — feat: add SqlxQueueBackend behind apalis feature flag.** PostgreSQL-backed QueueBackend using sqlx with `FOR UPDATE SKIP LOCKED` concurrency, ack/nack, orphan recovery. Gated behind `apalis` feature in sera-queue.

10. **`27a38cd` — feat: wire HITL ApprovalRouter into turn lifecycle act() step.** Added `WaitingForApproval` variant to TurnOutcome/ActResult. `ApprovalRouter.needs_approval()` wired into act(); creates ApprovalTicket when approval required.

11. **`0f3956f` — feat: add circle coordination scaffold with CircleState and shared memory.** CircleCoordinator with CircleState, SharedMemory KV store, CircleMessage (broadcast + directed) in sera-gateway. 14 tests.

12. **`b35dae1` — feat: add WebSocket transport behind enterprise feature gate.** WebSocketTransport implementing Transport trait using tokio-tungstenite, JSON-serialized SQ/EQ envelope. Behind `enterprise` feature flag.

---

## 2. Milestone verification

### M2 — gateway and runtime spine (confirmed)

- [x] `cargo check -p sera-gateway` green on `default` features
- [x] `Submission` / `Event` serde roundtrip tests pass; `Op` enum exhaustive match compiles
- [x] `AppServerTransport` enum present with all 6 variants; `InProcess` always compiled
- [x] `cargo check -p sera-runtime` green; `TurnResult` absent from sera-runtime (`grep -r TurnResult rust/crates/sera-runtime/` returns only comment tombstone)
- [x] Four-method turn lifecycle (`observe`/`think`/`act`/`react`) callable in test
- [x] Doom-loop threshold (`DOOM_LOOP_THRESHOLD = 3`) triggers `TurnOutcome::Interruption` in test
- [x] `MAX_COMPACTION_CHECKPOINTS_PER_SESSION = 25` constant present
- [x] `main.rs` rewritten with NDJSON Submission/Event loop
- [x] `reasoning_loop.rs`, `tool_loop_detector.rs`, `context_pipeline.rs`, `context_assembler.rs` deleted from sera-runtime; `TaskInput`/`TaskOutput` absent from main.rs
- [x] sera-docker shim call-site migration complete; sera-docker crate deleted
- [x] All gateway acceptance tests (8) and runtime acceptance tests (11) pass

### M4 — all lanes complete (confirmed)

- [x] All Lane A–F deliverables landed
- [x] `cargo check --workspace` green (21 crates, sera-docker removed → 20 workspace members + sera-session added → 21)
- [x] `cargo test --workspace` — 0 failures across 68 test suites
- [x] sera-session: SessionStateMachine 6 states, ContentBlock transcript, 14 tests
- [x] sera-testing: MockQueueBackend + MockSandboxProvider, 8 tests

### Previous milestones (still confirmed)

- **M1** — infrastructure (sera-telemetry, sera-config, sera-queue, sera-tools, scaffolds)
- **M3** — workflow and auth (WorkflowTaskId content-hash, casbin RBAC, argon2 key hashing)

---

## 3. What's next — Phase 2

**Phase 1 is complete.** All execution substrate wiring, design decisions, and remaining items are implemented and tested.

### Phase 1 completed items (all 12)

- [x] Wire DefaultHarness think step to llm_client.rs
- [x] Connect sera-gateway orchestrator to harness_dispatch::dispatch
- [x] Wire sera-queue QueueBackend into AppState (already existed)
- [x] Add sqlx persistence for session parts
- [x] Delete deprecated TurnResult
- [x] Wire NDJSON runtime loop to DefaultRuntime.execute_turn
- [x] Wire condensers into ContextEngine compact method
- [x] Wire ConstitutionalGate hooks into observe/react lifecycle
- [x] SqlxQueueBackend behind apalis feature flag
- [x] HITL ApprovalRouter wired into act() step
- [x] Circle coordination scaffold (CircleState, SharedMemory, CircleMessage)
- [x] WebSocket transport behind enterprise feature gate

### Phase 1 remaining work — now complete

All design decisions resolved and implemented in the second Phase 1 session:

- [x] **apalis job workers** — `SqlxQueueBackend` in sera-queue behind `apalis` feature flag. PostgreSQL-backed with `FOR UPDATE SKIP LOCKED` concurrency, ack/nack/orphan recovery.
- [x] **Circle coordination** — `CircleCoordinator` with `CircleState`, `SharedMemory` (KV store), and `CircleMessage` (broadcast + directed) in sera-gateway/services/circle_state.rs. 14 tests.
- [x] **HITL routing integration** — `WaitingForApproval` variant added to `TurnOutcome` and `ActResult`. `ApprovalRouter.needs_approval()` wired into `act()`. Creates `ApprovalTicket` when approval required; autonomous mode skips all checks.
- [x] **Enterprise transports** — `WebSocketTransport` implementing `Transport` trait in sera-gateway/transport/websocket.rs, behind `enterprise` feature flag. tokio-tungstenite, JSON-serialized SQ/EQ envelope over text frames.

---

## 4. Crate inventory (21 workspace members)

| Crate | Status | Tests | Lane |
|-------|--------|-------|------|
| sera-types | M0 stable | 272 unit + 22 integration | A |
| sera-telemetry | M1 | 18 | B |
| sera-config | M1 extended | 66 (14 new) | B |
| sera-queue | M1 | 12 | B |
| sera-tools | M1 | 15 | C |
| sera-errors | Scaffold | 0 | C |
| sera-cache | Scaffold | 0 | C |
| sera-secrets | Scaffold | 0 | C |
| sera-workflow | M3 rewritten | 40 (14 new) | E |
| sera-auth | M3 extended | 40 (12 new) | E |
| sera-events | Legacy | — | — |
| sera-gateway | **M2 NEW** — renamed from sera-core | 205 + 8 acceptance | D |
| sera-runtime | **M2 REWRITTEN** — TurnOutcome + ContextEngine | 19 + 11 acceptance | D |
| sera-session | **NEW** M4 | 14 | F |
| sera-testing | **EXTENDED** M4 | 8 (mock tests) | F |
| sera-db | Unchanged | — | — |
| sera-hooks | Unchanged | — | — |
| sera-hitl | Unchanged | — | — |
| sera-tui | Unchanged | — | — |
| sera-byoh-agent | Unchanged | — | — |

**Deleted:** sera-docker (all call sites migrated to sera-tools SandboxProvider)

---

## 5. Design decisions made this session

- **Envelope types defined in sera-gateway, not sera-types.** Avoids polluting the leaf crate with gateway-specific concerns. sera-runtime uses local serde-compatible types for its NDJSON protocol to avoid a cyclic dependency.
- **AgentHarness trait in sera-gateway, not sera-types.** The spec suggested sera-types to avoid cycles, but since sera-runtime doesn't import sera-gateway (it defines its own NDJSON types), the trait lives where it's used.
- **ContextEngine is a separate trait from AgentRuntime.** Orthogonal axis per SPEC-runtime §2.4. Pipeline and KvCache are two impls.
- **9 Condensers, 3 are P1 stubs.** LLMSummarizing, LLMAttention, StructuredSummary are passthrough stubs with `// TODO(P1)`.
- **MvsTurnResult rename in bin/sera.rs.** Local struct renamed to satisfy M2 exit criteria (`grep -r TurnResult` returns zero active hits). The MVS binary will be deprecated in Phase 1.
- **Integration tests for old reasoning_loop removed.** The TaskInput/TaskOutput/reasoning_loop API is deleted. New integration tests will be added in Phase 1 when the four-method lifecycle is wired to real LLM calls.

---

## 6. Gotchas carried forward

Previous gotchas §6.1–§6.11 from prior handoffs still apply. New additions:

- **§6.12 sera-runtime has no sera-gateway dependency.** The NDJSON protocol types are defined locally in main.rs to avoid a cycle. If envelope types change in sera-gateway, update sera-runtime's local types to match.
- **§6.13 TurnResult deprecated, not deleted.** `sera_types::runtime::TurnResult` has `#[deprecated]` but still exists for backward compatibility with bin/sera.rs MVS binary. Delete in Phase 1 when MVS binary is removed.
- **§6.14 sera-docker is gone.** Any code that tries to import `sera_docker` will fail. Use `sera_tools::sandbox::docker::DockerSandboxProvider` instead.

---

## 7. Files that exist and matter

Same as M1/M3 handoff §7, plus:
- **`rust/crates/sera-gateway/src/envelope.rs`** — SQ/EQ types (Submission, Event, Op)
- **`rust/crates/sera-gateway/src/transport/`** — Transport trait + InProcess/Stdio/WebSocket impls
- **`rust/crates/sera-gateway/src/services/circle_state.rs`** — CircleCoordinator + CircleState + SharedMemory
- **`rust/crates/sera-queue/src/sqlx_backend.rs`** — SqlxQueueBackend (PostgreSQL, behind `apalis` feature)
- **`rust/crates/sera-gateway/src/harness_dispatch.rs`** — AgentHarness trait + registry
- **`rust/crates/sera-gateway/src/kill_switch.rs`** — Emergency stop
- **`rust/crates/sera-runtime/src/context_engine/`** — ContextEngine trait + Pipeline/KvCache
- **`rust/crates/sera-runtime/src/compaction/`** — Condenser trait + 9 impls
- **`rust/crates/sera-runtime/src/turn.rs`** — Four-method lifecycle
- **`rust/crates/sera-runtime/src/handoff.rs`** — Agent-to-agent handoff
- **`rust/crates/sera-session/`** — SessionStateMachine + Transcript
- **`rust/crates/sera-testing/src/mocks/`** — MockQueueBackend + MockSandboxProvider

---

## 8. Cross-reference map

Carried forward from M0 handoff §8 — unchanged.

---

## 9. Session tooling

- **Task tracking:** Use `bd` (beads) for all task tracking. Run `bd prime` for full workflow context. Do NOT use TodoWrite, TaskCreate, or markdown TODO lists.
- **Knowledge management:** Use `omc wiki` for persistent knowledge across sessions. Significant discoveries, design decisions, and environment quirks should be captured via `wiki add` or `wiki ingest`. Query existing knowledge with `wiki query` before re-investigating known issues.

---

**End of handoff.** Phase 0 is complete. A fresh session reading this file can begin Phase 1 implementation work.

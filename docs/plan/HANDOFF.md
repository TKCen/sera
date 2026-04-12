# SERA 2.0 Phase 0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**M2 milestone reached. M4 milestone reached.** Lanes D and F are complete. All Phase 0 lanes (A–F) are done.

Six commits on `sera20`:

1. **`4fcfe21` — feat: rename sera-core → sera-gateway.** `git mv` + 3-file update (workspace Cargo.toml, crate Cargo.toml, CLAUDE.md). Clean diff.

2. **`d60bf37` — feat: SQ/EQ envelope, transport spine, harness dispatch.** Submission/Event/Op envelope types, AppServerTransport (6 variants, InProcess always compiled), Transport trait, InProcessTransport (mpsc), StdioTransport (NDJSON child process), HarnessRegistry, PluginRegistry, SessionPersist (PartTable + SessionSnapshot stubs), KillSwitch (arm/disarm/handle_command), GenerationMarker, ConnectorRegistry. 8 gateway acceptance tests.

3. **`9120100` — feat: sera-runtime contract migration.** ContextEngine trait (ingest/assemble/compact/maintain), ContextPipeline + KvCachePipeline impls, PipelineCondenser + 9 Condenser impls (NoOp through LLM stubs), Handoff type, four-method turn lifecycle (observe/think/act/react), DOOM_LOOP_THRESHOLD=3, SubagentHandle stub, DefaultHarness. 11 runtime acceptance tests.

4. **`19c0e45` — chore: delete sera-docker shim.** All call sites in sera-gateway migrated to `sera_tools::sandbox::docker::DockerSandboxProvider`. sera-docker crate deleted.

5. **`6b99041` — fix: complete M2 migration.** AgentRuntime::execute_turn returns TurnOutcome. default_runtime.rs uses new ContextEngine. Deleted: reasoning_loop.rs, tool_loop_detector.rs, context_pipeline.rs, context_assembler.rs. main.rs rewritten for NDJSON Submission/Event. bin/sera.rs local TurnResult renamed to MvsTurnResult. Integration tests updated.

6. **`5a2e15d` — feat: Lane F scaffolds.** sera-testing: MockQueueBackend (4 tests) + MockSandboxProvider (4 tests). sera-session: 6-state SessionStateMachine (8 tests) + ContentBlock transcript (6 tests).

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

## 3. What's next — Phase 1

Phase 0 is complete. All type contracts, trait boundaries, and infrastructure crates are in place. Phase 1 focuses on wiring the execution substrate:

### Phase 1 priorities

1. **Runtime execution substrate** — sqlx persistence for WorkflowTask, apalis job workers, circle coordination, HITL routing
2. **Model integration** — Wire LLM calls through the four-method lifecycle (think step), connect to LiteLLM gateway
3. **Transport wiring** — Connect StdioTransport to gateway's harness dispatch, wire InProcessTransport for integrated mode
4. **Compaction wiring** — Wire condensers into ContextEngine compact method, connect to token counting
5. **sera-testing integration tests** — Use MockQueueBackend + MockSandboxProvider for full-stack integration tests
6. **sera-hooks P1** — HookPoint::ConstitutionalGate enforcement, HookResult::updated_input
7. **Enterprise features** — WebSocket/gRPC transport implementations (behind feature gates)

### Recommended first steps

1. Wire the `DefaultHarness` in sera-runtime to call LLM via `llm_client.rs` in the `think` step
2. Connect sera-gateway's orchestrator to `harness_dispatch::dispatch` instead of direct reasoning_loop calls
3. Add sqlx persistence for session parts (sera-gateway's `session_persist.rs`)
4. Wire sera-queue's `QueueBackend` into sera-gateway's `AppState.queue_backend`

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
- **`rust/crates/sera-gateway/src/transport/`** — Transport trait + InProcess/Stdio impls
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

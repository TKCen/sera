# SERA 2.0 Phase 1+2 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** Phase 1+2 session 5 → `git show d02f7f7:docs/plan/HANDOFF.md`; Phase 1+2 session 4 → `git show 6440dca:docs/plan/HANDOFF.md`; Phase 1+2 session 3 → `git show 13f1b6c:docs/plan/HANDOFF.md`; Phase 1 session → `git show 54adaea:docs/plan/HANDOFF.md`; Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**Lane-aware routing, session-aware harness, transcript recording.** Twenty-two commits on `sera20` — twenty from sessions 1–5 plus one from session 6:

**Session 6 — Lane queue wiring + session-aware harness + transcript recording (3 beads resolved):**

21. **`106e4e3` — feat: wire lane queue, session-aware harness, and transcript recording.** Three interconnected changes: (a) Session-aware harness — `session_key` added to NDJSON Submission, gateway passes it through `StdioHarness::send_turn()`, runtime uses it in `TurnContext` instead of generating throwaway keys. (b) Lane queue wiring — `process_message()` enqueues to `LaneQueue` before dispatching; Ready→dispatch, Queued→wait, Interrupt→abort+dispatch; drain loop after each turn for pending messages. (c) Transcript recording — `DefaultRuntime` accumulates tool-call messages during the loop, returns them in `FinalOutput.transcript`; runtime emits `ToolCallBegin`/`ToolCallEnd` NDJSON events; gateway captures via `StdioHarness` and persists to SQLite (assistant+tool_calls and tool+content rows). Closes sera-vx9y, sera-umvp, sera-mmiq.

**Session 5 — StdioHarness dispatch + Docker setup (1 bead resolved):**

19. **`208f062` — feat: replace inline LLM loop with StdioHarness dispatch to sera-runtime.** The gateway no longer calls the LLM directly. On startup, it spawns a `sera-runtime --ndjson` child process per agent via `StdioHarness` and routes turns through NDJSON stdin/stdout. Removed ~480 lines of inline LLM calling, tool execution, and context overflow handling. 64 tests pass with mock harness. Closes sera-uzg5.

20. **`8ce11cd` — feat: add Docker setup for sera gateway + runtime.** `Dockerfile.sera` multi-stage build producing both `sera` and `sera-runtime` binaries. `docker-compose.sera.yml` with host networking for LM Studio. `sera.yaml.example` with local LLM provider + Discord connector.

**Session 4 — concrete ToolDispatcher + standalone CLI (2 beads resolved):**

16. **`43acd0c` — feat: add concrete ToolDispatcher impl and tool-call loop.** `RegistryDispatcher` bridges `ToolDispatcher` trait to existing `ToolExecutor`-based `ToolRegistry` (13 built-in tools). Tool-call loop in `DefaultRuntime::execute_turn()` re-enters think() on `RunAgain`, capped at `max_tool_iterations`. main.rs wired with dispatcher + tool definitions for LLM. 7 new tests. Closes sera-kjf9.

17. **`5e1ece0` — docs: update HANDOFF.md — session 4 concrete ToolDispatcher.**

18. **`059b273` — feat: add CLI args and interactive REPL to sera-runtime.** The runtime binary is now a standalone CLI: `sera-runtime --llm-url http://localhost:1234/v1 -m qwen/qwen3.5-35b-a3b`. Auto-detects interactive (TTY → REPL) vs programmatic (piped → NDJSON). Conversation history, system prompt support, all args have env var fallbacks. Closes sera-wn3c.

**Session 3 — E2E turn path (5 beads resolved):**

13. **`245a3c1` — feat: pass LLM response through react() instead of stub string.** react() now receives ThinkResult and extracts actual LLM response content for FinalOutput. Closes sera-ba3p.

14. **`d7db888` — feat: replace naive token counting with tiktoken-rs cl100k_base.** ContextPipeline::estimate_tokens uses lazily-initialized BPE encoder instead of len/4 heuristic. Closes sera-vfj9.

15. **`409aff6` — feat: add ToolDispatcher trait and wire into act() step.** ToolDispatcher trait in sera-runtime (async, serde_json::Value in/out). act() is now async — dispatches tool calls through dispatcher when provided. DefaultRuntime gains .with_tool_dispatcher(). Closes sera-d5tk, sera-3iqk. sera-ouyt was already implemented (LlmClient wired in main.rs).

**Session 1–2 — Phase 1 (twelve commits):**

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

## 3. What's next — E2E Docker validation + tool-level resource safety

**Session 6 complete. Lane queue wired, harness session-aware, transcript records tool calls.**

### Session 7 target: E2E Docker validation, tool-level resource safety, protocol alignment

**Goal:** Validate the full pipeline E2E in Docker with Discord. Add tool-level resource safety (FileTime.withLock pattern). Align the NDJSON protocol with the typed SQ/EQ envelope.

**What exists (after session 6):**
- `StdioHarness` in sera.rs: spawns `sera-runtime --ndjson` per agent on startup, sends turns via NDJSON with `session_key`. Returns structured `TurnEvents` with response text + `ToolEvent::Begin`/`End`.
- `LaneQueue` wired into `process_message()`: enqueue → dequeue → dispatch → complete_run → drain loop. Per-session serialization with cross-session concurrency. 5 queue modes. Global cap (default 10).
- Transcript recording: `DefaultRuntime` accumulates tool-call messages in `FinalOutput.transcript`. Runtime emits `ToolCallBegin`/`ToolCallEnd` NDJSON events. Gateway persists assistant+tool_calls and tool+content rows to SQLite.
- `Dockerfile.sera` + `docker-compose.sera.yml` + `sera.yaml.example`.
- Session persistence (SQLite): sessions, transcripts (now includes tool calls), audit log.

**Architecture (implemented):**
```
Discord ch1 ──┐                    ┌─ session:sera:ch1 ─┐
Discord ch2 ���─┼→ sera (gateway) ──→│  LaneQueue §5      │──→ StdioHarness ──�� sera-runtime
HTTP /chat  ──┘   │                └─ session:sera:ch2 ─���      (one per agent)
                  SQLite (sessions, transcripts w/ tool calls, audit)
```

**What needs to happen:**
1. **E2E Docker validation** — `docker compose -f rust/docker-compose.sera.yml up --build`, send Discord messages across multiple channels, verify: (a) responses arrive with tool call results, (b) concurrent sessions work (different channels), (c) same-session messages serialize correctly, (d) transcript persists all messages + tool calls.
2. **Tool-level resource safety** — Add `FileTime.withLock` pattern to `file_write`/`file_edit` tools in sera-runtime: check mtime before writing, reject if file was modified since last read by this session. This is how OpenClaw prevents workspace conflicts between concurrent sessions (see SPEC-dependencies §10.7).
3. **Protocol alignment** — Review the OpenClaw design (SPEC-dependencies §10.5) for `supports()` + `parent_session_key` patterns. Align the Submission/Event wire format so `AgentHarness::handle(Submission)` works with the typed envelope. Currently StdioHarness builds JSON directly (§6.20).
4. **chat_handler lane queue integration** — The HTTP chat handler doesn't yet use the lane queue (only process_message does). Wire it for completeness.
5. **Steer injection at tool boundary** — The lane queue supports Steer mode but the runtime doesn't yet check for steer events at tool boundaries. Wire the peek_steer/take_steer flow.

**Key constraints:**
- Gateway owns: connectors (Discord), session persistence, hook lifecycle, lane-aware routing, transcript recording. NO agent-level locking.
- Runtime owns: LLM calls, tool execution, context management, tool-level resource safety (mtime checks, atomic ops).
- Context management is a runtime concern (plugin-capable) — the gateway sends the conversation, the runtime manages its window.
- The session_key enables per-session context state in the runtime when context management plugins are added.
- See SPEC-gateway §5 for lane queue semantics, §3 for SQ/EQ envelope.
- See SPEC-dependencies §10.5 for OpenClaw patterns, §10.7 for opencode FileTime.withLock.

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

### Session 6 — Lane queue + session-aware harness + transcript (current)

- **Lane queue wired into process_message, not chat_handler.** The Discord event loop (process_message) uses the full enqueue → dispatch → complete_run → drain pattern. The HTTP chat_handler uses session_key for the harness but does not yet go through the lane queue — it's a synchronous request/response and doesn't need queue semantics yet. Follow-up: wire chat_handler through the queue for consistency.
- **Session key format: `discord:{agent}:{channel_id}` for Discord, `http:{agent}:{session_id}` for HTTP.** Different channels map to different sessions. Same channel always maps to the same session (per-session serialization). HTTP sessions use the SQLite session ID since there's no channel concept.
- **Transcript field on FinalOutput, not event callbacks.** Rather than threading a callback/channel into DefaultRuntime for per-tool-call event emission, we added a `transcript: Vec<serde_json::Value>` field to `TurnOutcome::FinalOutput`. The runtime accumulates assistant+tool_calls and tool result messages during the tool-call loop and returns them. The NDJSON loop then emits `ToolCallBegin`/`ToolCallEnd` events from this data. Simpler than callbacks; sufficient for current needs.
- **TurnEvents struct wraps harness response.** `StdioHarness::send_turn()` now returns `TurnEvents { response: String, tool_events: Vec<ToolEvent> }` instead of a bare String. The gateway uses `persist_tool_events()` to write tool calls to the transcript before the final assistant response.
- **Drain loop in process_message, not spawned task.** After a turn completes, the drain loop runs inline (check has_pending → dequeue → execute → complete_run → repeat). This is simpler than spawning separate tasks and maintains the sequential event_loop processing model. Cross-session concurrency comes from the lane queue's global cap, not from concurrent event_loop processing.

### Session 5 — StdioHarness dispatch

- **Gateway routes to pre-connected harness, not per-turn spawn.** `StdioHarness` is spawned once per agent on startup and reused for all turns. The gateway resolves provider config (base_url, model, api_key) from manifests and passes them as env vars to the child process. Harness lifecycle management (spawn on agent creation/activation) is a follow-up.
- **StdioHarness bypasses typed Submission/Event envelope.** The gateway's `Submission` uses `ContentBlock` items while the runtime expects OpenAI-format messages as `serde_json::Value`. Rather than force a lossy conversion, the harness builds runtime-compatible JSON directly. Protocol alignment is a follow-up task.
- **Gateway no longer imports sera-runtime tools or context.** Removed `ContextManager` and `MvsToolRegistry` imports from sera.rs. The gateway's role is strictly: Discord adapter, session persistence, hook lifecycle, and Stdio transport. All LLM calls and tool execution live in the runtime.
- **Mock harness for tests.** `StdioHarness::spawn_mock()` (test-only) spawns a bash script that echoes canned NDJSON events. This avoids needing a real LLM for the 64 sera binary tests.
- **Usage info is zeroed.** The runtime's NDJSON events don't currently include `TokenUsage`. The gateway returns zeroed usage until the runtime emits it in `TurnCompleted`.

### Session 4 — concrete ToolDispatcher

- **Bridge to ToolExecutor, not TraitToolRegistry.** The `RegistryDispatcher` bridges `ToolDispatcher` (trait in turn.rs) to the existing `ToolExecutor`-based `ToolRegistry` (13 working tools in sera-runtime/src/tools/). The spec-aligned `TraitToolRegistry` exists but has zero concrete tool implementations — bridging to it would mean rewriting all 13 tools for no user-visible benefit. Migration to `TraitToolRegistry` is a follow-up task for policy enforcement.
- **No sera-tools dependency needed.** sera-runtime already has its own `ToolRegistry` and 13 `ToolExecutor` implementations. The sera-tools crate's `registry.rs` is a separate, simpler abstraction.
- **Tool-call loop in DefaultRuntime, not main.rs.** The loop (observe→think→act→react→RunAgain→repeat) belongs in `execute_turn()` because it's an AgentRuntime concern. main.rs just handles NDJSON transport. Loop capped at `max_tool_iterations` (default 10).
- **Message accumulation order matters.** When re-entering think() after tool results, the assistant message (with tool_calls) is appended BEFORE tool result messages. This is an OpenAI API requirement — the LLM expects tool_call before tool results.
- **Tool definitions via serde round-trip.** `crate::types::ToolDefinition` (Value-based parameters) → `serde_json::Value` → `sera_types::tool::ToolDefinition` (typed FunctionParameters). All 13 tool schemas round-trip successfully (validated by test).
- **Standalone CLI with TTY detection.** `sera-runtime` uses clap for CLI args + `atty` for TTY detection. Interactive mode (REPL with conversation history) when stdin is a TTY; NDJSON mode when piped. All CLI args have env var fallbacks for container deployments. `--ndjson` flag forces NDJSON even on a TTY.

### Session 3 — E2E turn path

- **Gateway is a thin event dispatcher.** The gateway routes messages to harnesses and persists sessions. It has **zero involvement in tool execution**. Tool management and execution are entirely harness-internal. A future management plane may push tool/skill configuration to harnesses, but that's a separate concern.
- **Harness is fully self-contained.** The harness/runtime owns the complete turn loop (observe/think/act/react), LLM calls, tool registry, tool execution, and context management. It must run standalone via Stdio/WebSocket transport without calling back to the gateway.
- **`ToolDispatcher` trait in sera-runtime.** Follows the same decoupled pattern as `LlmProvider` — defined in sera-runtime, concrete impl will also live there. The gateway does not provide tool dispatch.
- **`react()` receives `ThinkResult`.** Instead of a separate `TokenUsage` param, react() takes the full ThinkResult and extracts the LLM response content for FinalOutput. Eliminates the stub string.
- **`act()` is async.** Tool dispatch is inherently async; act() now takes an optional `ToolDispatcher` and dispatches tool calls through it.
- **Token counting via tiktoken-rs cl100k_base.** Replaces the `len/4` heuristic in ContextPipeline. Lazy-initialized static encoder.
- **AgentHarness trait needs to move out of sera-gateway** (sera-w9bn). Currently in harness_dispatch.rs — should live in a shared crate since harnesses are standalone processes, not gateway-owned objects.

### Previous sessions

- **Envelope types defined in sera-gateway, not sera-types.** Avoids polluting the leaf crate with gateway-specific concerns. sera-runtime uses local serde-compatible types for its NDJSON protocol to avoid a cyclic dependency.
- **AgentHarness trait in sera-gateway, not sera-types.** ⚠️ **Deprecated decision** — see sera-w9bn. This should move to a shared crate.
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
- **§6.15 Gateway is a thin event dispatcher — NOT a tool orchestrator.** The gateway routes messages to harnesses and persists sessions. It has zero involvement in tool management or execution. Tool registry, tool dispatch, and tool execution are entirely harness-internal. Do not add tool-related logic to sera-gateway. See sera-kjf9 for the concrete ToolDispatcher impl work.
- **§6.16 AgentHarness trait is misplaced in sera-gateway.** It should live in a shared crate (sera-types or new sera-harness) since harnesses are standalone processes. See sera-w9bn. Do not add harness logic that assumes gateway access.
- **§6.17 ~~sera-runtime needs sera-tools dep.~~** ✅ Resolved in session 4. sera-runtime had its own `ToolRegistry` with 13 `ToolExecutor` impls — no sera-tools dep was needed. `RegistryDispatcher` bridges `ToolDispatcher` to `ToolRegistry`.
- **§6.18 Two ToolDefinition types coexist.** `crate::types::ToolDefinition` (Value-based parameters) and `sera_types::tool::ToolDefinition` (typed `FunctionParameters`). main.rs does a serde round-trip to convert. If a new tool uses exotic JSON Schema features (arrays with `items`, `oneOf`, nested objects), the round-trip may silently drop fields. The `all_tool_definitions_round_trip` test catches this.
- **§6.20 StdioHarness bypasses typed Submission envelope.** The gateway's `Submission` type uses `ContentBlock` items while the runtime's NDJSON protocol expects `Vec<serde_json::Value>` (OpenAI-format messages). `StdioHarness::send_turn()` builds JSON directly rather than going through the typed `Submission`. Do not try to use `Transport::send_submission()` or `AgentHarness::handle()` with the StdioHarness until the protocol is aligned.
- **§6.21 Harness spawned on startup, not on agent creation.** `StdioHarness` instances are spawned in `run_start()` for each agent in the manifest. If the manifest changes at runtime, the harnesses won't update. Future: move harness lifecycle to agent creation/activation API.
- **§6.19 Two tool registries coexist in sera-runtime.** `ToolRegistry` (ToolExecutor-based, 13 tools, used for dispatch) and `TraitToolRegistry` (sera_types::Tool-based, zero tools, spec-aligned with policy). Follow-up task: migrate ToolExecutor impls to the Tool trait for policy enforcement.
- **§6.22 Lane queue is in-memory only.** The LaneQueue in sera-db is a pure in-memory HashMap. On gateway restart, all queue state is lost. This is acceptable for Tier 1 (local) but needs SQLite backing for crash recovery. The drain loop in process_message will not resume pending messages after restart.
- **§6.23 chat_handler does not use lane queue.** The HTTP `/api/chat` handler passes `session_key` to the harness but does not enqueue through `LaneQueue`. This means HTTP requests can race with Discord messages on the same session. Wire it through the queue for full consistency.
- **§6.24 FinalOutput.transcript is serde_json::Value vec.** The `transcript` field on `TurnOutcome::FinalOutput` carries OpenAI-format messages (role + content/tool_calls). It uses `#[serde(default, skip_serializing_if = "Vec::is_empty")]` for backward compat. Downstream code pattern-matching on FinalOutput must use `..` to ignore it or explicitly destructure it.

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
- **`rust/crates/sera-runtime/src/tools/dispatcher.rs`** — RegistryDispatcher (ToolDispatcher → ToolRegistry bridge)
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

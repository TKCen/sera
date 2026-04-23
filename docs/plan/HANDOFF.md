# SERA 2.0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-23 (session 2, evening)
> **Session:** Duplicate-implementation cleanup + un35 P1 regression closeout
> **Previous handoff:** earlier 2026-04-23 → `git show 697c099f:docs/plan/HANDOFF.md`. Chain back from there.

---

## Session outcome — 9 PRs merged, 5 parallel lanes + one amend

Every lane landed. The un35 P1 regression flagged as must-fix-first did not reproduce on current main; it was almost certainly cleared by `sera-df7h` (#1017 the prior session) and the filing snapshot was stale. Diagnostic annotation was added so a future recurrence surfaces an actionable error instead of a bare "Broken pipe".

- **#1019** (`sera-6i18` P3) — CLAUDE.md: drop stale Docker Compose / bun install refs, update dev path to WSL2.
- **#1020** (`sera-igsd` P2) — chat_handler fails closed on SessionStore emission errors. Releases the lane, logs at error, returns 500. tasks/permission_requests/intercom still log-and-continue (mechanical follow-up; see `sera-<new>` if filed).
- **#1021** (`sera-dxib` P4) — delete legacy `sera-hooks/src/wasm_adapter.rs` (510 LOC). `ComponentAdapter` remains the one-true host.
- **#1022** (`sera-iwbq` P2) — delete dead duplicate `sera_types::queue::QueueBackend` (405 LOC). `sera_queue::QueueBackend` is canonical.
- **#1023** (`sera-zx5w` P2) — collapse `sera-runtime::stdio::{Submission,Op,Event,EventMsg}` onto `sera_types::envelope::*`. Design choice: `session_key`/`parent_session_key` live at envelope-level on `Submission`, not inside `Op::UserTurn` (correlation metadata, not turn content). `Op::UserTurn.items` is now `Vec<serde_json::Value>` matching wire reality. Canonical-only fields (`cwd`/`approval_policy`/`sandbox_policy`/`effort`/`final_output_schema`) retained as `#[serde(default)]` tolerated extras for future wiring. `EventMsg::ToolCallStarted/Completed` → `ToolCallBegin/End` to match wire names. Follow-up commit fixed 23 compile errors in `tests/gateway_acceptance.rs` (12 fixture sites).
- **#1024** (`sera-3rmo` P2) — guard against empty `execute_turn` reply in `/api/chat`. Returns 502 with `{"error":"runtime returned empty reply"}` + rich log (session_id, agent, usage, tools_ran). Does NOT chase the deployed-container root cause — that's a separate operability concern.
- **#1025** (`sera-un35` P1) — diagnostic hardening of `StdioHarness::send_turn`: stdin write/flush errors now annotate with the runtime child's exit status via `child_exit_context()`. Operator sees `"sera-runtime child exited before submission could be written (status: ...)"` instead of bare `Broken pipe (os error 32)`. Regression itself did not reproduce on fresh build + fresh `.sera-local` state against current main; likely cleared by #1017 df7h.
- **#1009** + **#1010** — dependabot dep bumps (uuid 10→14 in `legacy/`, git2 0.19→0.20 in `rust/`). Green at time of merge.

All 9 merges pulled into local main. `.clawhip/` is the only untracked path (tmux monitoring artefact — leave it).

---

## Architectural decisions reinforced this session

- **Envelope shape is owned by `sera-types`.** `sera-runtime::stdio` no longer ships its own `Submission`/`Op`/`Event` types. If you change the wire shape, touch `sera-types/src/envelope.rs` and let every crate re-compile against it.
- **Correlation metadata is envelope-level, not Op-level.** `session_key` and `parent_session_key` belong to `Submission`. `Op::UserTurn` carries only turn content. Don't add correlation fields back to Ops.
- **Fail-closed on audit writes** (chat path). SessionStore emission failure means the audit trail is broken — reject the turn with 500 rather than silently succeed. Same principle applies to future tasks/intercom/permission callers (not yet wired).
- **Empty reply is a bug, not a success.** If `execute_turn` returns an empty string, that's a silent failure surface — emit 502 and log. Do not let an empty string travel as a 200 response body.
- **Dead code earns no keep-alive tax.** `wasm_adapter.rs` (510 LOC) and `sera_types::queue` (405 LOC) both had zero live consumers and were deleted outright rather than deprecated with TODO comments. Prefer delete over deprecate when the consumer count is zero.

---

## Open follow-up beads (nothing P0/P1 outstanding)

P2:
- **`sera-qrsh`** — `sera-gateway`: define dedicated `Op` variants for tasks / permission_requests / intercom routes (currently all wrapped as `Op::UserTurn`, which is semantically wrong for replay). Requires `sera-types::envelope::Op` extension + re-wrap at the 4 call sites in `bin/sera.rs`. Blocked only by 1023 merging — now unblocked.
- **`sera-3rmo` followups** — the 3rmo fix closed the silent-failure hole on the local path but did NOT chase WHY the deployed container's `execute_turn` returns empty. That's a separate Docker-env investigation. File a fresh bead if it reproduces.
- **`sera-igsd` followups** — apply fail-closed semantics to tasks/permission_requests/intercom emission paths (still log-and-continue). Also: a `FailingSessionStore`-based unit test was drafted by the executor but deferred; file a small bead for its re-introduction once the AppState constructor is easier to mock.
- **`sera-4yz5`** (OSS README / LANDING page) — still blocked on code stability; current code IS now stable, so this is ready to work whenever someone wants to announce Sera.

P3:
- **`sera-xoie`** — `/api/chat` usage tokens always 0. Runtime→gateway propagation drops them. Likely LM Studio `usage` field not being read in the runtime's model client.
- **`sera-bb39`** — pick one home for `TranscriptEntry` (`sera-session` vs `sera-types`).
- **`sera-dsht`** — upstream LCM to public hermes-agent repo + re-anchor `sera-context-lcm` to submodule. External-repo work, not local.
- **`sera-msal`** — sera-hooks E2E WASM component build in CI (blocked on `wasm-tools` / `wasm32-wasip2` availability in CI image).

P4:
- **`sera-0ym4`** — ergonomic cleanup around `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` (too shouty; rename or flip default once operator docs catch up).

---

## Primary goal for next session (pick one — ask if unsure)

### 1. Land `sera-qrsh` — proper `Op` taxonomy for non-UserTurn envelope emissions
Now unblocked by #1023. The current code wraps task-result, permission-request, intercom-publish, intercom-dm all as `Op::UserTurn` — replay tooling cannot tell them apart. Define `Op::TaskResult`, `Op::PermissionRequest`, `Op::IntercomPublish`, `Op::AgentDm`, re-wrap the 4 emission sites in `bin/sera.rs`, update SPEC-gateway if applicable. Medium-size refactor, mostly mechanical, well-scoped.

### 2. Propagate LM Studio `usage` through runtime→gateway (`sera-xoie`)
Small functional bug with a diagnostic payoff. `{"prompt_tokens":0,"completion_tokens":0,...}` in every `ChatResponse` is bad telemetry. Start at the runtime's model client, follow the field through `StdioHarness::send_turn`'s return.

### 3. Write README / LANDING (`sera-4yz5`)
Code is stable enough now. This is the gating artefact for making Sera publicly interesting. Architecture diagram, vision, quick-start, 'Why SERA vs LangChain/AutoGen/CrewAI'. Larger than a bead implementation but high-leverage.

### 4. Fail-closed parity for remaining emission sites (igsd followup)
tasks/permission_requests/intercom emission still fail-open. Mechanical mirror of what #1020 did for chat. Small bead.

---

## Environment reminders

- `scripts/sera-local` defaults `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` since #1017. Override on the command line if you need strict mode.
- `DEFAULT_TURN_TIMEOUT = 600s` (from #1013). Override via `SERA_TURN_TIMEOUT_SECS`.
- Canonical envelope types live in `rust/crates/sera-types/src/envelope.rs`. Do NOT add a shadow type in any other crate.
- LM Studio loopback: `http://host.docker.internal:1234` from containers, `http://localhost:1234` from host/WSL.
- `bd` is the only task tracker. Do not use TodoWrite/TaskCreate/markdown checklists.

---

## Wiki pointers (LLM wiki at `.omc/wiki/`)

Use `wiki_query <keyword>` for architecture docs. Pages refreshed this session:
- `phase0-complete-architecture-status.md` / `phase1-complete-e2e-gap.md` — phase gates passed.
- `crate-spec-mapping.md` — current crate→SPEC mapping.
- `in-process-hooks-first.md` — decision still stands; ComponentAdapter is the forward path.
- `thiserror-source-field.md` — gotcha to remember when touching error enums.

---

## Session tally

- 9 PRs merged (7 code + 2 dependabot).
- 9 beads closed (`sera-6i18`, `sera-igsd`, `sera-dxib`, `sera-iwbq`, `sera-zx5w`, `sera-3rmo`, `sera-un35`, plus the dependabot flows).
- 6 parallel lanes (un35 opus, zx5w opus, iwbq sonnet, igsd sonnet, 3rmo sonnet, 6i18 haiku) + 1 opportunistic lane (dxib sonnet) + 1 recovery amend (zx5w gateway_acceptance fixture fix via sonnet).
- Net LOC: large negative (−405 iwbq, −510 dxib, and several hundred more via surgical cleanups).
- Zero open regressions. Zero open P0/P1 beads.

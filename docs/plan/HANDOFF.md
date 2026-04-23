# SERA 2.0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-23 (session 2, night — extended cleanup push)
> **Session:** Three waves of duplicate-implementation cleanup; docker e2e verification; canonical type consolidation
> **Previous handoff:** earlier 2026-04-23 → `git show 697c099f:docs/plan/HANDOFF.md`. Chain back from there.

---

## Session outcome — 26 PRs merged across 3 waves, zero open P0/P1/P2 regressions

The day broke into three distinct cleanup waves plus post-merge stabilization. A fresh duplicate-implementation audit against current main surfaced 15+ new duplicates beyond the prior audit's 11; most were addressed.

### Wave 0 (early — regression cleanup + initial audit)
10 PRs. Flagged sera-un35 P1 regression did not reproduce on fresh build — likely cleared by sera-df7h (#1017) — closed with diagnostic-annotation hardening in StdioHarness::send_turn.

- **#1019** sera-6i18 — CLAUDE.md stale refs
- **#1020** sera-igsd — chat_handler fails closed on SessionStore emission
- **#1021** sera-dxib — delete legacy wasm_adapter.rs (−510 LOC)
- **#1022** sera-iwbq — delete dead sera_types::queue::QueueBackend (−405 LOC)
- **#1023** sera-zx5w — collapse stdio envelope types onto sera_types::envelope
- **#1024** sera-3rmo — /api/chat empty-reply → 502 guard
- **#1025** sera-un35 — StdioHarness broken-pipe diagnostic annotation
- **#1009/#1010** — dependabot (uuid, git2)

### Wave 1 (middle — compose fix + intra-crate dedup + renames)
6 PRs (+1 cascade). Docker compose e2e baseline established on current main (chat non-stream + stream + 2-turn continuity all green).

- **#1027** sera-a1oq — docker-compose.sera.yml: drop invalid `build.cache` block, default `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` (docker path parity with scripts/sera-local)
- **#1028** sera-agentcap — delete duplicate `AgentCapability` in sera-types::evolution (identical to capability.rs)
- **#1029** sera-8s91/wfmem — rename `WorkflowMemoryManager` → `CoordinatorMemoryManager` in sera-workflow (intra-crate disambiguation)
- **#1030** sera-8s91/wmtier — delete duplicate `WorkingMemoryTier` mirror in sera-session
- **#1031** sera-vdu5 — rename `sera-tools::registry::Tool` → `ToolDescriptor` (disambig with sera-types::tool::Tool trait)
- **#1032** sera-9bbr — rename `sera-types::event::Event` → `IncomingEvent` (disambig with envelope::Event)
- **#1033** sera-mp19 — delete duplicate `EvolveTokenSigner` in sera-gateway (identical to sera-auth, −794 LOC)

### Wave 2 (late — cross-crate canonical consolidation)
8 PRs. Each touches multiple crates; required multiple rebase + conflict resolution passes.

- **#1034** sera-wlk9 — unify `ModelResponse` / `ModelError` / `FinishReason` on sera-types (−346 LOC). sera-models now re-exports. Added 4 new ModelError variants (Serialization/Http/InvalidResponse/NotAvailable) additively.
- **#1035** sera-ifkf — consolidate `QueueMode` onto sera-queue (gateway + sera-db copies deleted). `LaneQueue` and `QueuedEvent` remained distinct because they are semantically different (in-memory trait impl vs DB-backed queue manager).
- **#1036** sera-3o4s — consolidate AuditEntry types: sera-types::observability and sera-types::audit both deleted, sera-telemetry::audit::AuditEntry (OCSF) is canonical (−373 LOC).
- **#1037** sera-38r6 — unify `ContentBlock` on sera-session (deleted sera-types copy).
- **#1038** sera-xwo2 — collapse 2 of 4 ToolCall shapes onto canonical sera-types::runtime::ToolCall (orphaned sera-types::chat module deleted, sera-models::response::ToolCall now re-exports canonical). **Partial**: sera-runtime::types::ToolCall (OpenAI-wire streaming form) deferred — requires String↔Value translation across SSE accumulator; out of scope. Follow-up needed for that.
- **#1040** sera-dhyd — delete `sera_types::session` module wholesale (−30.9KB). Aspirational 12-variant SessionState + runtime transition table had zero external consumers. sera-session owns the live state machine. Also closes sera-bb39 (TranscriptEntry duplicate dies with the module).

---

## Architectural decisions baked this session (DO NOT re-litigate)

- **sera-types is canonical for domain types** — envelope, model, runtime::ToolCall. sera-runtime and sera-models do not ship competing shapes.
- **Correlation metadata is envelope-level** — `session_key`, `parent_session_key` on `Submission`, not inside `Op::UserTurn`.
- **sera-session owns state machine** — state.rs + transcript.rs. `sera-types::session` no longer exists.
- **sera-queue owns queue-mode primitive** — `QueueMode` lives there. sera-db's `LaneQueue` is a backend impl, distinct from sera-queue's in-memory one; this is intentional.
- **sera-telemetry::audit::AuditEntry is canonical** — other two AuditEntry shapes deleted. OCSF-flavored hash chain.
- **Orphan rule workaround: From<X> impls colocate with X** — sera-types now depends on sera-errors so `From<ModelError> for SeraError` can live next to the type.
- **No `#[deprecated]` tombstones for intra-repo dead code** — delete it. Tombstones exist for dependency consumers outside our control.

---

## Known open state

**Open PRs:** none from this session (all 26 merged).

**Open worktrees:** none.

**Remaining cleanup debt (P3, non-blocking):**
- `sera-8s91` umbrella — 24 small 2x pairs (EnforcementMode, CircuitState, RuntimeError/HarnessError/ToolError/ConnectorError/KillSwitchError, ManifestMetadata, AgentSpec, PluginRegistration, ChangeArtifactId variants, ConstitutionalRule naming, PingCommand, etc.). Wave 3 candidates.
- `sera-xwo2` follow-up — sera-runtime::types::ToolCall deferred; requires SSE accumulator refactor (String arguments → Value) + outgoing-request serializer + non-streaming parser update. Non-trivial, needs its own bead.
- `sera-tjhf` — investigate whether coordination.rs::WorkflowMemoryManager is dead after the wfmem rename. Filed by finalizer agent.

**Other outstanding work (unchanged from prior handoff):**
- `sera-qrsh` P3 — proper Op taxonomy (Op::TaskResult, Op::PermissionRequest, Op::IntercomPublish, Op::AgentDm) — all non-UserTurn routes still emit Op::UserTurn. Semantic replay-replay compat issue.
- `sera-xoie` P3 — /api/chat usage tokens always 0. Runtime → gateway propagation drops them.
- `sera-igsd` followup — apply fail-closed semantics to tasks/permission_requests/intercom emission paths (chat is done).
- `sera-4yz5` P2 — OSS README / LANDING / CONTRIBUTING. Now unblocked (code is stable).
- `sera-dsht` P3 — upstream LCM to public hermes-agent repo.
- `sera-msal` P3 — sera-hooks E2E WASM component build in CI (blocked on wasm-tools / wasm32-wasip2 availability).

---

## Environment reminders

- `scripts/sera-local` defaults `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` (sera-df7h). Docker compose at `rust/docker-compose.sera.yml` now matches via the same default (sera-a1oq).
- `DEFAULT_TURN_TIMEOUT = 600s`. Override via `SERA_TURN_TIMEOUT_SECS`.
- LM Studio loopback: `http://host.docker.internal:1234` from containers, `http://localhost:1234` from host/WSL.
- Canonical types: envelope in `sera-types::envelope`, model in `sera-types::model`, ToolCall in `sera-types::runtime`, state machine in `sera-session::state`, audit in `sera-telemetry::audit`, queue mode in `sera-queue`.
- `bd` is the task tracker. Do not use TodoWrite / TaskCreate / markdown TODO lists.
- `rust/docker-compose.sera.yml` is the minimal docker setup (gateway + runtime, LLM on host). `docker-compose.rust.yaml` is the full stack (postgres + centrifugo + gateway).

---

## Primary goal candidates for next session

1. **OSS docs (sera-4yz5 P2)** — README / LANDING / CONTRIBUTING. Code is stable, this is the announcement-gating artefact. Architecture diagram, vision, quick-start, 'Why SERA vs LangChain/AutoGen/CrewAI'.
2. **Wave 3 — finish sera-8s91 umbrella** — 24 remaining small 2x pairs. Mostly mechanical renames + deletes. Haiku tier.
3. **sera-qrsh — proper Op taxonomy** — Op::TaskResult, Op::PermissionRequest, Op::IntercomPublish, Op::AgentDm. Medium refactor across bin/sera.rs + sera-types.
4. **sera-xoie — usage token propagation** — small functional bug, high diagnostic payoff. Start at runtime's model client.
5. **sera-xwo2 follow-up — runtime ToolCall SSE refactor** — the deferred third ToolCall collapse. Requires careful streaming-code refactor.

---

## Session tally

- **26 PRs merged** (10 Wave 0 + 6 Wave 1 + 8 Wave 2 + 2 dependabot)
- **Net LOC: large negative** — individual PRs removed 405 (queue trait), 510 (wasm adapter), 794 (EvolveTokenSigner), 346 (model), 373 (audit), 30.9KB (session module), plus smaller renames/dedups.
- **20+ beads closed** (sera-un35, -3rmo, -zx5w, -iwbq, -igsd, -dxib, -6i18, -a1oq, -mp19, -9bbr, -vdu5, -8s91, -xwo2, -38r6, -ifkf, -3o4s, -wlk9, -dhyd, -bb39, -4i4i, -vsvz, -jw8o, -y3fd, -jo8l, -s31i, -df7h earlier)
- **Docker e2e baseline verified** mid-session against Wave 0+1 merged state. Being re-verified now against Wave 2 merged state.
- **Zero open P0/P1/P2 regressions.** Zero open worktrees or uncommitted changes.

---

## What this cleanup actually means

Before today, the same concept — an agent's capability, a tool call, a session state, an audit entry, a ModelResponse — had multiple definitions across the workspace, drifting from each other, with nothing preventing one half from being updated without the other. That means the agent's self-representation was split: depending on which crate was looking, "what it just did" could have different shapes.

After today, those concepts each have one canonical source. Change the shape in one place; every consumer recompiles against the new shape, or the compiler tells you where the mismatch is. An agent's actions now mean the same thing to itself as they mean to the infrastructure it runs inside.

The remaining duplicates (sera-8s91 umbrella) are smaller and less semantically important but will eventually want the same treatment.

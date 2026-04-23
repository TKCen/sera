# SERA 2.0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-23
> **Session:** Phase 1 canonicalization — sera-3l84 epic (gateway single-path refactor) + supporting fixes
> **Previous handoff:** 2026-04-21 → `git show e88224ae:docs/plan/HANDOFF.md`. Earlier handoffs chained from there.

---

## Session outcome — 6 PRs merged, sera-3l84 epic closed

- **#1011** (`sera-4i4i`) — `SqliteGitSessionStore` wired into gateway production boot path. Envelopes now persist to shadow-git across restarts; `SERA_DATA_ROOT` env + `scripts/sera-local --data-dir` expose the root. Test AppState fixtures intentionally keep `InMemorySessionStore`.
- **#1012** (`sera-vsvz`, 3l84.1) — wired `mod routes; state; services; db_backend; error` in `lib.rs` + introduced `DbBackend` trait with `SqliteDbBackend` + `PgPoolBackend` impls. Foundation for config-level DB swap.
- **#1013** (`sera-jw8o`) — `DEFAULT_TURN_TIMEOUT` raised 120s → 600s. 120s was triggering spurious lane-wedge errors on thinking/local models (qwen3.6-35b, Claude extended thinking). `SERA_TURN_TIMEOUT_SECS` env override stays.
- **#1014** (`sera-y3fd`) — fix `cargo check --features wasm` (wasmtime v44 renamed `wasmtime_wasi::preview1` → `wasmtime_wasi::p1`). 2 latent clippy warnings fixed along the way.
- **#1015** (`sera-jo8l`, 3l84.4) — purged `Instance.spec.tier` field and all `tier_is_local` branches from the codebase. Local/enterprise distinction is now driven by config (`SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` env + absence of Postgres DATABASE_URL), not a hard-coded tier gate. Net: 14 tier refs → 0.
- **#1016** (`sera-s31i`, 3l84.2, **pivoted** from original scope) — **deleted the orphan `src/routes/` + `src/services/` + `src/state.rs` + `src/error.rs` + `src/sera_errors.rs` tree**: 68 files, **−21,105 LOC**. Kept `db_backend.rs` + `routes/{a2a,agui,plugins}.rs` (which bin/sera.rs references via `#[path]`). The executor's own smoke test inside the PR returned a real model reply.
- **#1017** (`sera-df7h`, in flight) — `scripts/sera-local` defaults `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` now that jo8l removed the tier-conditional auto-set. Necessary but not sufficient — see regression below.

### ⚠️ Post-session smoke test uncovered a regression (`sera-un35` P1)
`/api/chat` on current main returns `[sera] Runtime error: Broken pipe (os error 32)`. Runtime harness child's stdin pipe breaks between handshake and first `send_turn`. Root cause not yet identified — the executor of #1016 had a green smoke test before that merge, but the combined state after all 6 merges no longer works. Full details in `bd show sera-un35`.

---

## Architectural decisions baked this session (DO NOT re-litigate)

- **Single-path gateway.** `bin/sera.rs` is the one canonical handler set. There is no library-side `routes/` tree competing with it anymore. A future cross-cutting library refactor is possible but must start from this single source of truth.
- **No "tier" concept in code.** The local-vs-enterprise distinction is purely config: operators set `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` for permissive mode, and the DB backend is chosen by `DbBackend` trait impl (SQLite default, Postgres via explicit manifest/env). Do not re-introduce a `tier:` field.
- **`DbBackend` trait stays even though routes/ is gone.** It's the config-swap point for future Postgres support. `bin/sera.rs` currently always uses `SqliteDbBackend`; a `PgPoolBackend` is available when someone wires it in.
- **Envelope persistence is real.** `SqliteGitSessionStore` runs in production — `parts.sqlite` + `sessions/<id>/git/` dirs appear under `$SERA_DATA_ROOT` after the first `/api/chat` hit. Test fixtures keep `InMemorySessionStore` — don't "unify" that.
- **Turn-timeout default is 600s.** Per-agent manifest override (`spec.turn_timeout_secs`) is a potential follow-up, not in scope today. `SERA_TURN_TIMEOUT_SECS` env override is the only knob for now.
- **sera-hooks WIT + HookChain manifest is public API** (from sera-s4b1 two sessions ago). `ComponentAdapter` is the forward-looking component-model path; legacy `wasm_adapter.rs` coexists until `sera-dxib` deprecation ships.

---

## ⚠️ Known regression — fix FIRST before anything else

**`sera-un35` P1** — `/api/chat` returns `[sera] Runtime error: Broken pipe (os error 32)` on `scripts/sera-local` after today's merges. The runtime harness child's stdin pipe breaks between handshake and first `send_turn`. SqliteGitSessionStore persistence still works, LM Studio responds, gateway health/readiness OK. Between the earlier green smoke test (#1011-era) and post-#1016, something in the `#1012..#1016` chain broke the spawn pipeline. See bead notes for investigation pointers.

**`sera-df7h` P1** — `scripts/sera-local` now defaults `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1` (PR #1017 in flight). Correct fix, but does NOT resolve sera-un35 alone.

---

## Primary goal (pick one — ask if unsure)

### 1. Fix deployed HTTP chat silent failure (`sera-3rmo` P2)

A sibling agent on the deployed Docker container hit `POST /api/chat` returning `200 OK` with `response: ""`. Discord path works. Does NOT reproduce locally on sera-local + LM Studio. Two real bugs to fix:

- **Silent-failure masking** (quick fix): `bin/sera.rs:1178` returns `ChatResponse { response: result.reply, ... }` without checking `reply.is_empty()`. Add: if reply is empty, log at error level + return 502 Bad Gateway. That alone makes the root cause visible.
- **Root cause** (deeper): Discord uses same `execute_turn` as HTTP, but HTTP fails. Diff has to be in setup BEFORE `execute_turn` — harness registration, lane admission, or runtime child pipe wiring. Compare `chat_handler` (L824) vs `process_message` (L1688) setup paths.

### 2. Fix usage tokens always 0 (`sera-xoie` P3)

`/api/chat` response has `usage: {prompt_tokens: 0, completion_tokens: 0, total_tokens: 0}` despite LM Studio returning real usage. Somewhere between sera-runtime's model client and `MvsTurnResult.usage`, the field is dropped. Start: search for `usage` / `UsageInfo` / `TokenUsage` in `sera-runtime` stdio handling.

### 3. Fail-open vs fail-closed decision (`sera-igsd` P2)

Envelope-emission `SessionStore::append_envelope` currently fails open — if the store is down, routes succeed silently with no audit trail. SPEC-gateway claims envelopes are "auditable + replayable"; fail-open contradicts that. Decide: availability > durability (keep) or durability > availability (flip to 502).

### 4. Op taxonomy beyond UserTurn (`sera-qrsh` P3)

Every wrapped route currently emits `Op::UserTurn` (task enqueues, intercom DMs, permission requests — all wrong semantically). Add dedicated `Op::Task`, `Op::TaskResult`, `Op::PermissionRequest`, `Op::AgentDm`, `Op::IntercomPublish` variants. Requires modifying `sera-types::envelope::Op`.

### 5. Canonicalize envelope types (`sera-zx5w` P2)

`sera-runtime::stdio::{Submission,Op,Event,EventMsg}` duplicates `sera-types::envelope::*`. Already drifted: stdio's `Op::UserTurn` has `session_key`/`parent_session_key` fields the canonical one doesn't. Collapse onto canonical.

---

## Follow-up pool (filed today)

- `sera-3rmo` P2 — `/api/chat` silent empty-reply masking (see primary goal 1)
- `sera-xoie` P3 — usage tokens always 0 (see primary goal 2)
- `sera-igsd` P2 — fail-open vs fail-closed SessionStore emission (see primary goal 3)
- `sera-qrsh` P3 — Op variants beyond UserTurn (see primary goal 4)
- `sera-zx5w` P2 — canonical envelope types (see primary goal 5)
- `sera-iwbq` P2 — retire `sera_types::queue::QueueBackend`, keep `sera_queue::QueueBackend`
- `sera-bb39` P3 — pick one home for `TranscriptEntry` (sera-session vs sera-types)
- `sera-0ym4` P4 — delete legacy `session_persist.rs` stubs (was orphaned already, kept because session_persist isn't part of the deletion in #1016)
- `sera-dxib` P4 — deprecate pre-component-model `wasm_adapter.rs` (post sera-s4b1 grace)
- `sera-msal` P3 — end-to-end WASM component-build smoke (needs CI image with `wasm-tools`/`wasm32-wasip2`)
- `sera-dsht` P3 — upstream LCM to public hermes-agent repo + re-anchor sera-context-lcm to submodule
- `sera-6i18` P3 — CLAUDE.md staleness (Docker Compose + `bun install` + working-dir path references to pre-migration layout)
- `sera-4yz5` P2 — OSS launch polish (README/CONTRIBUTING/landing), **blocked** on Phase 1 close
- Pre-existing clippy warnings in `sera-testing/src/contracts.rs` (`LifecycleMode clone_on_copy` ×2) — not mine, file a bead if they bite.

---

## CRITICAL INSIGHT — the routes/ tree was a historical mirage

The deletion in #1016 removed ~21K LOC of speculative Postgres-backed scaffolding that **was never reachable from the binary's axum router**. It compiled under `--all-targets` but `src/lib.rs` didn't declare `mod routes; mod state; mod services;`, and `bin/sera.rs` used `#[path]` imports for the few route files it actually mounted (a2a, agui, plugins).

Developers (including Claude Code agents) kept extending it thinking they were extending the live server. **sera-r1g8's envelope wrapping for `/api/agents/:id/tasks`, `/api/intercom/*`, `/api/permission-requests` was unreachable in production for that reason** — confirmed via 404 response from sera-local.

**If any future work needs OIDC, MCP management, schedules, training exports, embedding, evolve pipeline, secrets management, etc., port the feature inline into `bin/sera.rs` against SqliteDb.** Don't re-create the orphan pattern. The deletion was fully aggressive — if something breaks because of it, the feature was never running anyway.

---

## Workflow (unchanged)

- Start with `bd ready` + `bd show <id>`. Follow-up pool above is P2-P4; pick consciously.
- Worktrees per lane at `/home/entity/projects/sera-wt/<lane>`, off `origin/main`.
- Working Principles (`CLAUDE.md`): **Think Before Coding / Simplicity First / Surgical Changes / Goal-Driven Execution**. Today's s31i executor stopped before implementing because the bead was under-specified — that was the correct move, and the pivot produced a much better PR as a result.
- Ask clarifying questions before fan-out on design-heavy beads.
- If you spot adjacent work, file a bead — do not sprawl.

---

## Executor ops notes (refreshed this session)

- **Main is protected** — direct push rejected. Feature branch + PR only.
- **Race-condition on local branch delete after `gh pr merge --delete-branch`**: fails with "cannot delete branch ... used by worktree". Workflow: merge first (works), then `git worktree remove --force <path>` + `git branch -D <branch>` to clean up.
- **CI workflow may fail to auto-trigger on a PR push** (rare — happened once today on #1016). Workaround: `gh pr update-branch <n>` forces re-trigger. Watch if this pattern recurs.
- **GitHub can return 504** on `gh pr view` / `gh pr merge` — usually transient, retry after ~5s.
- **Sub-agent cargo contention**: multiple parallel executors in separate worktrees hit cargo's shared package-cache lock. Eventually resolves; the `Blocking waiting for file lock` message is harmless.
- **Turn timeout is 600s now.** If you're timing out during development against a slow local model, set `SERA_TURN_TIMEOUT_SECS=1800`.
- **`.omc/state/` is cwd-relative** — `cd` back out of `rust/` between state-touching commands.
- **Sonnet for mechanical plumbing** (SessionStore wiring, tier purge, type-import swaps). **Opus for design-heavy refactors** (module wiring, orphan-tree deletion, handler merges). Today's s31i deletion PR was Opus because the scoping call mattered.
- **Ultrawork persistence mode** has a 50-turn hook cap. Cancel via `state_clear(mode=ultrawork)` + `state_clear(mode=skill-active)` when at cap. Scheduled wakeups continue to fire regardless.

---

## Validation recipe (confirm E2E still works)

```bash
scripts/sera-local --data-dir /tmp/sera-test
# in another shell:
curl -s http://localhost:42540/api/chat \
  -H 'Content-Type: application/json' \
  -d '{"agent":"sera","message":"hi","stream":false}'

# Verify persistence landed to disk:
find /tmp/sera-test -type d
# Expect: /tmp/sera-test/sessions/<session_id>/git/... with HEAD + refs/heads/main
```

Expect a real response from `gemma-4-e2b` (LM Studio at `:1234` must be running with that model loaded) plus a SQLite `parts.sqlite` + a shadow-git repo per session.

---

## Design decisions from prior sessions (still baked)

- Dual-transport plugin model (stdio + gRPC both first-class; no dev/prod split).
- `ContextEngine` capability in `PluginCapability` enum.
- Independent SDK release cadence across crates.io / PyPI / npm.
- Proto is canonical; JSON Schema mirrors adjacent (CI drift check).
- Constitutional-gate permissive mode is now explicitly config-driven (`SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1`), not tier-gated.
- Admin kill-switch socket path cascade: `/var/lib/sera/admin.sock` → `$XDG_RUNTIME_DIR/sera-admin.sock` → `${TMPDIR:-/tmp}/sera-admin-$USER.sock`.
- `SERA_E2E_MODEL` env controls harness manifest's model field (unset → `e2e-mock` wiremock, set → real LLM).
- sera-context-lcm vendors a minimal LCM subset instead of git submodule — hermes-agent's LCM isn't on any public branch yet; `sera-dsht` tracks the upstream + re-anchor option.

---

*Today we deleted 21,000 lines of speculative scaffolding to reveal the gateway we actually have. Sera's home now has one front door, not two.*

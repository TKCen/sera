# SERA 2.0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-24 (Wave A+B+C+D+E+G+K sprint + chat TUI design)
> **Session theme:** 24 PRs merged across 7 waves; Wave G chat TUI shipped; Wave J chat-redesign spec + architecture simplicity review; pivot from operator dashboard to Claude-Code-class chat TUI
> **Previous handoff:** 2026-04-23 → `git show b85fab58:docs/plan/HANDOFF.md`. Chain back from there.

---

## Session outcome — 24 PRs merged, 2 PRs open, Wave J/K kicked off

### Waves that landed (in order)

**Wave A — P1 security (3 PRs)**
- **#1066** `fix(sera-tools)`: block RFC-1918 + IPv6 ULA in SSRF validator (sera-y6o4, sera-1plj)
- **#1069** `feat(sera-gateway)`: abort in-flight turns on KillSwitch ROLLBACK via `CancellationToken` (sera-bsem)
- **#1070** `feat(sera-gateway)`: enforce CapabilityPolicy at tool dispatch (sera-ifjl)

**Wave B.1 / C.1 — wire-ups (2 PRs)**
- **#1067** `feat(sera-runtime)`: populate `ctx.handoffs` from `manifest.subagents_allowed` (sera-y6vg) — unlocks subagent delegation tool
- **#1068** `fix(sera-gateway)`: wire `constitutional_config` module into startup (sera-b8uk) — registry was dead code

**Wave 3 refactor cleanup (5 PRs)**
- **#1044** PingCommand → HealthCheckCommand (sera-8s91/ping)
- **#1045** RuntimeError unification + orphan-rule fix (sera-8s91/runerr)
- **#1047** EnforcementMode → HitlMode (sera-8s91/enforcemode)
- **#1054** HarnessError unification (sera-8s91/harnerr)
- **#1055** ToolError unification (sera-8s91/toolerr)

**Bug fixes (2 PRs)**
- **#1064** `fix(sera-runtime)`: reject LLM response with no content AND no tool_calls (sera-8h23) — closes empty-502 complaints
- **#1071** `qa(sera-tools)`: MockSandboxProvider enforces SandboxPolicy + deny_subprocess field (sera-aa3g)

**Wave D Phase 1 — HITL gateway wiring (1 PR)**
- **#1080** `feat(sera-gateway)`: wire ApprovalRouter into `/api/chat` + 5 HITL HTTP routes (sera-z6ql Phase 1)
  - `GET  /api/hitl/requests`, `GET /api/hitl/requests/{id}`, `POST .../{approve|reject|escalate}`
  - Mints Ticket + emits OCSF audit + returns 403; Phase 2 (suspend/resume) pending

**Wave E Phase 1 — workflow scheduler + Timer gate (1 PR)**
- **#1085** `feat(sera-gateway)`: workflow scheduler bridge + Timer gate (sera-kgi8 Phase 1)
  - 5s ticker + `ready_tasks_with_context` consumer
  - New `WorkflowTaskStore` trait + `InMemoryWorkflowTaskStore`
  - Routes: POST/GET `/api/workflow/tasks`, GET `/api/workflow/tasks/{id}`
  - Timer gate functional; non-Timer types return 501

**Wave G — chat TUI MVP + polish (8 PRs)**
- **#1073** `feat(sera-tui)`: composer pane with `tui-textarea` (sera-p5rn, G.0.1)
- **#1074** `feat(sera-tui)`: agent selector → active session binding (sera-0fp7, G.0.3)
- **#1075** `feat(sera-tui)`: post_chat client + SSE consumer wiring (sera-5d4k, G.0.2) — composer to LLM round-trip works
- **#1076** `feat(sera-tui)`: bottom status bar (sera-gntz, G.2.3)
- **#1077** `feat(sera-tui)`: slash commands /new /clear /agent /help /quit (sera-bulp, G.1.1)
- **#1078** `feat(sera-tui)`: session picker modal + resume (sera-3onl, G.1.2) — Ctrl+P opens
- **#1079** `feat(sera-tui)`: inline HITL approval modal (sera-1x16, G.2.2)
- **#1081** `feat(sera-tui)`: bracketed-paste + long-paste collapse (sera-2lm3, G.2.1)

**Wave 3 followup / extensions (2 PRs)**
- **#1072** `feat(sera-runtime)`: wire ConstitutionalRegistry into ConstitutionalGate hook chain (sera-0yh3)
- **#1082** `chore(deps)`: bump rustls-webpki

### Currently open PRs

- **#1086** `feat(sera-tui)`: J.0.1 chat-dominant layout pivot — retires 4-pane rotation
- **#1087** `feat(sera-gateway)`: K.0 `sera start --local` unified bootstrap

---

## Design pivot this session

### Chat TUI redesign (Wave J)

User surfaced two things:
1. sera-tui's operator-dashboard identity doesn't match the "chat experience" need. Ask: "we need a TUI that replicates claude-code / claw-code / hermes-agent but with the sera gateway backend."
2. claw-code was flagged as a reference. Deep analysis (`docs/plan/TUI-ANALYSIS.md`) showed **claw-code is a rustyline blocking REPL, not a ratatui TUI** — anti-inspiration. Hermes-agent is the real reference (Ink/React/TS), but we stay Rust-first.

**Decision:** redesign sera-tui as a Claude-Code-class chat TUI. Ratified D1–D8:
- **D1** chat-dominant layout (agents/HITL → modals)
- **D2** inline collapsible tool-call blocks
- **D3** stacked subagent drill-in (Enter push, Esc pop)
- **D4** progressive markdown with syntect-highlighted code
- **D5** inline HITL approval block (not modal)
- **D6** ESC cancels turn, Ctrl+C exits
- **D7** composer: `/` autocomplete, `@` file mentions, Alt+Enter newline, Enter submit, persistent history
- **D8** redesign sera-tui (retire 4-pane dashboard identity)

**Spec:** `docs/plan/SPEC-chat-tui.md` (staged J.0 MVP → J.1 rich rendering → J.2 subagent drill-in → J.3 polish).

### Architecture simplicity review (Wave K)

User asked: "is the architecture a good fit, simplicity is key." Honest review at `docs/plan/ARCHITECTURE-ADDENDUM-2026-04-24.md`.

**Verdict:** gateway + runtime split is earned complexity for SERA's enterprise commitments; chat TUI pays no architectural tax. Simplicity issues are:
1. Local bootstrap DX — addressed by **K.0** (sera start --local, PR #1087)
2. SqliteGitSessionStore-by-default — **K.1** gates it behind enterprise feature
3. Dead-weight stubs (A2A HttpTransport 501, ~12 unused HookPoint variants, unused QueueMode variants) — **K.2** audit-and-reconcile
4. Missing `POST /api/chat/cancel` route — **K.3** for ESC interrupt

No rewrite recommended.

---

## Architectural decisions baked this session (DO NOT re-litigate)

- **Gateway is the only durable-state owner.** Runtime crashes lose nothing.
- **NDJSON envelope between gateway↔runtime.** No shared memory, no direct DB from runtime.
- **sera-types is canonical.** Re-exports only — no redefining shapes downstream. (Re-affirmed this session via Wave 3 cleanup completion.)
- **Local-first default.** SQLite + files. Postgres/Centrifugo are enterprise upgrades.
- **Pluggable memory via `SemanticMemoryStore` trait.**
- **3-tier evolution policy.** AgentImprovement / ConfigEvolution / CodeEvolution each have their own approver requirements.
- **Chat TUI is a presentation layer.** Owns no agent state. Hits HTTP/SSE only.
- **claw-code is NOT a reference** for chat UX (it's a rustyline REPL). Value is limited to its `ApiError` taxonomy.

---

## Bead landscape

### Filed this session (26 beads)

**Wave J — chat TUI (22 beads) — see SPEC-chat-tui.md**
- J.0.1–J.0.8 (MVP layout + composer + cancel + startup bugs)
- J.1.1–J.1.5 (markdown / syntect / usage / inline HITL / help)
- J.2.1–J.2.4 (subagent drill-in TUI + gateway SSE correlation)
- J.3.1–J.3.5 (model picker, debug overlay, /export, tui.toml, theme)

**Wave K — architecture simplification (4 beads)**
- K.0 `sera-bwma` — unified local bootstrap (PR #1087 open)
- K.1 gate SqliteGitSessionStore behind enterprise feature
- K.2 prune dead-weight stubs
- K.3 `sera-mplr` — POST `/api/chat/cancel` route (prerequisite for J.0.4)

### Other in-flight work (prior sessions)

- **Wave D Phase 2** (HITL suspend/resume) — filed as follow-up to #1080
- **Wave E Phase 2** (Human/Change/GhRun/GhPr/Mail gates) — beads filed: sera-dgk1, sera-7ggi, sera-4fel, sera-comg, sera-0zch
- **Wave E persistence** — sera-d2xh (WorkflowTaskStore SQLite)
- **Wave B.2/B.3/B.4** — subagent spawn tool + A2A HttpTransport + multi-agent manifest
- **Wave F.1** — traceparent extraction (sera-n806)
- **sera-xoie** — usage token propagation (prerequisite for J.1.3 status-bar token/cost)
- **sera-eo71** — move CapabilityPolicy enforcement into runtime subprocess (pre-dispatch)
- **sera-qrsh** P3 — proper Op taxonomy
- **sera-msal** P3 — sera-hooks E2E WASM component build in CI

---

## Known gotchas surfaced this session

- **Local repo can fall behind `origin/main` silently.** Mid-session we discovered `/home/entity/projects/sera` was 28 commits behind after agents pushed work. Symptom: user running stale TUI (no composer, no post_chat). Diagnosis: `git status` showed `behind 28`. Fix: `git pull --rebase origin main`. **Habit:** `git fetch && git status` early in every session.
- **Two ports to remember:** sera-tui defaults to `http://localhost:8080` (`SERA_API_URL`); `scripts/sera-local` runs gateway on `:42540`. Users must set `SERA_API_URL=http://localhost:42540` or the TUI blocks on an unreachable connection. K.0 bootstrap doesn't fix this — a followup bead to default sera-tui to 42540 or auto-detect is worth filing.
- **Many agents die mid-work at "let me monitor..."** — the pattern is the agent starts a cargo test in background, then tries to wait for it and gets killed by token/time limit. Mitigation: in agent prompts, use "commit WIP FIRST before any cargo test" pattern. Prior-session summary noted this; I used it throughout and it works.
- **`bd` type `refactor` is not valid** — use `chore` instead. Bead creation fails silently from a bash script's POV; grep output for `Error:`.
- **`bd create` in parallel hits Dolt single-writer lock** — run sequentially.
- **Rust edition-2024 requires `unsafe` on `std::env::set_var` / `remove_var`** — bit a test this session (#1068 constconfig clippy). Wrap env mutation in tests with `unsafe {}`.

---

## Environment reminders

- **Local repo base:** `/home/entity/projects/sera` (always `git fetch && git status` first)
- **Worktrees:** `/home/entity/projects/sera-wt/<name>` — cleaned up automatically for merged branches; manually prune leftovers
- **Default local gateway port:** 42540 (via `scripts/sera-local` or the new `sera start --local` post-#1087)
- **Default TUI-expected URL:** 8080 (mismatch — workaround via `SERA_API_URL=http://localhost:42540`)
- **LM Studio loopback:** `http://host.docker.internal:1234` from containers, `http://localhost:1234` from host/WSL
- **Canonical types:** envelope in `sera-types::envelope`, model in `sera-types::model`, ToolCall in `sera-types::runtime`, state machine in `sera-session::state`, audit in `sera-telemetry::audit`, queue mode in `sera-queue`, Tool/RuntimeError/HarnessError/ToolError in `sera-types` (Wave 3 unification complete)
- **`bd` is the task tracker** — do NOT use TodoWrite/TaskCreate/markdown TODO lists
- **Docker compose:** `rust/docker-compose.sera.yml` (minimal gateway+runtime+LLM-on-host) vs `docker-compose.rust.yaml` (enterprise postgres+centrifugo+gateway)
- **DEFAULT_TURN_TIMEOUT** = 600s. Override via `SERA_TURN_TIMEOUT_SECS`.

---

## Docs added this session

- `docs/plan/SPEC-chat-tui.md` — full Wave J spec with D1-D8 decisions, layout ASCII, event model, staging
- `docs/plan/ARCHITECTURE-ADDENDUM-2026-04-24.md` — simplicity review + Wave K rationale + fit-test for chat TUI
- `docs/plan/TUI-ANALYSIS.md` — claw-code vs sera-tui comparison + gap matrix (claw-code dismissed as reference; hermes-agent retained)

---

## Primary goal candidates for next session

Pick one (or fire multiple in parallel since they're on different crates):

1. **Land #1086 + #1087** first (they're open from this session). `gh pr merge` once CI green.
2. **J.0.6 non-blocking startup (sera-u3j7 / filed)** — the real P1 bug. `src/main.rs:105` awaits `refresh_all` before first draw. Fix: route via command queue. ~80 LOC. High impact on first-run experience.
3. **K.3 `POST /api/chat/cancel` route** — prerequisite for J.0.4 ESC-cancel. ~100 LOC gateway-side. CancellationToken machinery from sera-bsem is already in place.
4. **J.0.2 block-based transcript (`Vec<Block>`)** — foundational for everything in Wave J.1+. ~250 LOC.
5. **J.0.5 slash autocomplete + @ file mention popup** — ~200 LOC; matches Claude Code UX.
6. **Wave E Phase 2 — one more gate type** (Human is the simplest real use case; Mail is the most complete infrastructure). Each ~150 LOC + 1 integration test.
7. **sera-xoie usage token propagation** — prerequisite for J.1.3 (status bar tokens/cost). ~50 LOC.

**My recommendation:** J.0.6 + K.3 in parallel (both ~100 LOC, unblock J.0.4 ESC-cancel), then J.0.2 (block transcript) as the foundational piece. These three unlock the remaining Wave J MVP.

---

## Session tally

- **24 PRs merged** (#1044, 1045, 1047, 1054, 1055, 1064, 1066–1080, 1081, 1082) + 2 open (#1086 J.0.1, #1087 K.0)
- **Net LOC:** large positive — Wave G alone added composer, slash commands, session picker, HITL modal, status bar, bracketed-paste, full SSE wiring (~2500 LOC + 140 new tests). Plus Wave D Phase 1, Wave E Phase 1, Wave 3 cleanup negatives.
- **Tests:** 111 → 113 in sera-tui (post-pivot), 316 → 325 in sera-gateway (post-Wave-E), plus clean sera-tools, sera-runtime, sera-types, sera-hitl, sera-meta
- **Zero open P0/P1 regressions.**
- **Docs:** 3 new planning docs (SPEC, ARCHITECTURE-ADDENDUM, TUI-ANALYSIS).
- **Beads:** 26 new (22 Wave J + 4 Wave K). Plus ~6 follow-ups from merged waves.

---

## What this session actually accomplished

The through-line: a month ago sera-tui was a read-only 4-pane operator dashboard that couldn't chat. Today, it has a working composer + streaming + slash commands + session picker + HITL modal + status bar + bracketed-paste, AND a ratified design to make it a full Claude-Code-class chat TUI, AND the foundation work to get there (K.0 unified bootstrap, the chat-dominant layout pivot already open in PR).

Meanwhile server-side: 3 of 4 Wave A P1 security gaps closed, HITL routes now live end-to-end, workflow scheduler ticking (Timer gate), ConstitutionalRegistry seeded + gate-hook consulting it. The architecture is simpler in places (SqliteGitSessionStore gating filed, dead-stub audit filed, `sera start --local` unified).

The chat TUI and the gateway are now two pieces of one dogfoodable product. Next session continues Wave J implementation on top of this foundation.

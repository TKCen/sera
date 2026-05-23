# SERA ↔ Hermes Parity Matrix

> **Status:** Living document. Phase 0 artifact from the SERA ↔ Hermes parity
> plan (`docs/internal/plans/SERA-HERMES-PARITY-DIFFERENTIATION-PLAN-2026-05-23.md`).
> Public-safe: do not paste private chat content, credentials, home-network details, or local-only paths into this file.

This matrix turns the parity plan from prose into a tracked acceptance list.
Each row describes one parity surface, the Hermes-side benchmark, the
current SERA path, the SERA target shape (including the differentiation
hook that keeps SERA accountable rather than just convenient), the
acceptance smoke / test, the bead that owns the work, and the matrix
status.

The reader of this matrix should be able to answer three questions:

1. Which Hermes parity gaps still force Sera to fall back to Hermes today?
2. Which SERA bead owns each open gap?
3. Which live smoke proves the baseline is green?

Status values:

- `BASELINE` — wired and proven by the live baseline gate
  (`rust/crates/sera-e2e-harness/tests/hermes_parity_baseline.rs`).
- `IN_PROGRESS` — bead is open and active.
- `GAP` — bead is open but unstarted, or no canonical bead yet.
- `DIFF_PRESERVED` — SERA path already exceeds Hermes on the
  differentiation axis (audit, policy, HITL, GoalRun).

Priority follows the bead's priority (`P1` … `P3`); rows without an owning bead are tagged with a recommended priority.

---

## Live baseline gate

The smallest acceptance bundle that proves SERA can host a turn at all:

| Probe | Surface | Expectation |
|---|---|---|
| Readiness | `GET /api/health/ready` | HTTP 200 and `runtime_connected=true`. |
| Authenticated chat nonce | `POST /api/chat` | HTTP 200 with `session_id` and a non-empty `response` that echoes the per-test nonce (freshness guard against stale/canned replies). The request carries `Authorization: Bearer`; auth is only enforced under an auth-enabled harness profile, tracked as a follow-up. |
| Response sanitizer contract | `POST /api/chat` response body | `response` contains no `<think>` or `</think>` markers (raw chain-of-thought is never leaked to the operator). |
| Skip contract | All probes | If gateway/runtime binaries or an LLM are not available, the gate skips cleanly with an explicit stderr line rather than failing. |

Owning crate: `rust/crates/sera-e2e-harness`. Owning test:
`tests/hermes_parity_baseline.rs` (gated behind `--features integration`).
Owning report template: `docs/internal/sessions/hermes-parity-baseline-template.md`
(template for operator-run live captures; the test itself is the canonical
CI/regression gate). The `artifacts/` working directory is gitignored, so
operator-run capture instances are kept out-of-tree by convention.

---

## Parity matrix

Columns:

- **Capability** — Hermes benchmark surface.
- **SERA current path** — what exists in the workspace today, including the crate / module that owns it.
- **SERA target path** — the parity-complete shape, including the differentiation hook (audit, policy, HITL, GoalRun, workflow, gateway-dispatch).
- **Acceptance smoke** — the smallest live or test-suite probe that proves parity.
- **Owner bead** — canonical bead anchor; create a new bead only when none exists.
- **Status** / **Priority**.

| # | Capability | SERA current path | SERA target path (with differentiation hook) | Acceptance smoke | Owner bead | Status | Priority |
|---|---|---|---|---|---|---|---|
| 1 | API chat intake (`/api/chat`) | `sera-gateway` chat handler (`crates/sera-gateway/src/bin/sera.rs::chat_handler`); session-scoped through gateway-owned SQLite. | Stable JSON contract (`session_id`, `response`, `usage`), SSE streaming events, `POST /api/chat/cancel`, response sanitizer applied before operator-visible bytes. **Differentiation:** gateway-owned session and audit, no runtime-side completion text reaching operator without sanitization. | `hermes_parity_baseline.rs::hermes_parity_baseline_gate` — POST with `Authorization: Bearer` returns `session_id` + a `response` that echoes the per-test nonce (freshness guard). Auth-enforced harness profile is a follow-up (see recommended new beads below). | `sera-duo3` (CI smoke pipeline), reinforced by `sera-yj18` (streaming tool-call edge cases). | BASELINE | P2 |
| 2 | Discord intake / egress | `sera-gateway` Discord connector (`crates/sera-gateway/src/discord.rs`, `bin/sera.rs::handle_message`); peer-mention handoff already reversed and stdio transport recycled per readiness probe. | Reliable buffered ingress + typed reactions/typing + visible failure replies + audit per Discord turn. **Differentiation:** rate-limited, audit-logged, capability-policy-checked Discord turns rather than raw bot loops. | Discord smoke fixture pair under `sera-duo3` once `sera-yeg.2` / `sera-s77a` land; manual smoke covered in `docs/internal/sessions/hermes-parity-baseline-template.md`. | `sera-yeg.2` (typing/reactions), `sera-s77a` (buffering / rate-limit), `sera-duo3` (CI smoke). | IN_PROGRESS | P2 |
| 3 | Provider / fallback conformance | `sera-runtime` OpenAI-compatible client + provider chain (MiniMax primary + local Qwen fallback) merged in #1272. | Ordered provider chain, typed failure classes, no silent fallback for policy/tool/schema errors, redaction of provider-internal errors before operator-facing output. **Differentiation:** routing is an accountable policy decision, not a config toggle. | `cargo test -p sera-runtime --lib provider_chain` + live MiniMax smoke documented in `docs/internal/sessions/hermes-parity-baseline-template.md`. | `sera-m1k8` (primary anchor) + `sera-yj18` (streaming tool-call edge cases). | IN_PROGRESS | P1 |
| 4 | Response sanitization / strict output | `sera-gateway` chat boundary applies [`response_sanitizer::sanitize_assistant_response`](../../../rust/crates/sera-gateway/src/response_sanitizer.rs) to non-streaming `/api/chat` replies (and the persisted transcript) before any operator-visible bytes are emitted. `sera-runtime` may keep raw text in logs/audit for debugging. | Hard contract: no `<think>` / `</think>` markers in operator-visible `response`; `reasoning_content` never bleeds into assistant text unless explicitly enabled. **Differentiation:** sanitizer is a contract enforced by the gateway boundary, not a hope. Streaming SSE path is the open follow-up — deltas may split a `<think>` tag across chunks, so a stream-aware sanitizer is tracked separately. | `hermes_parity_baseline.rs::hermes_parity_baseline_gate` (Probe 3 — passes vacuously with a non-reasoning mock) **plus** `hermes_parity_response_sanitizer.rs::hermes_parity_response_sanitizer_strips_think_blocks` (regression — mock LLM deliberately emits `<think>…</think>` and the gateway must strip it). | `sera-yj18` (current streaming tool-call empty-content boundary that drove the sanitizer hardening); `sera-jzsn` filed for the SSE-streaming sanitizer follow-up (relates_to `sera-m1k8`). | BASELINE | P2 |
| 5 | File / search / patch tools | `sera-tools` (`crates/sera-tools/src/mvs_tools.rs` + sibling tool modules); registered via the Tool trait. | Hermes-equivalent file / search / patch tools with mtime conflict checks, model-visible failure semantics, and gateway-dispatch authority. **Differentiation:** CapabilityPolicy + audit ledger per call. | New parity smoke under `sera-e2e-harness` once a tool-loop bead lands; tracked under `sera-tqzd` and `sera-q66q`. | `sera-tqzd` (failure surfacing), `sera-q66q` (audit ledger). New bead recommended for file/patch parity coverage. | GAP | P2 |
| 6 | Terminal / process tools | `sera-tools` terminal/process tool surface (current MVS subset); sandbox policy partial. | Hermes-equivalent terminal/process tools with sandbox policy, approval gates, and timeout / process-tree containment. **Differentiation:** HITL approval + sandbox boundary + audit per process spawn. | Negative-test smoke for denied tool / sandbox violation under `sera-tqzd`; new bead recommended for the positive-path parity smoke. | `sera-tqzd` (failure surfacing) + new bead to be created for terminal-tool parity. | GAP | P2 |
| 7 | Web / browser / MCP | MCP bridge scaffolding in `sera-tools` / `sera-runtime`; browser parity not yet wired. | Web search + browser + MCP bridge tools available with consistent failure semantics. **Differentiation:** egress policy + SSRF guardrails + audit. | Smoke that drives an MCP tool round-trip from `sera-runtime`; tracked under a new bead recommended below (no current anchor). | New bead recommended (working name: `sera-mcp-parity`). | GAP | P2 |
| 8 | Memory / session search | Tier-1 MemoryBlock + Tier-2 semantic recall scaffolding in `sera-runtime/src/context_engine`; gateway-owned durable memory in `sera-db`. | Two-tier injection: compact MemoryBlock + on-demand semantic search tool; session transcript search; cross-session operator state with provenance. **Differentiation:** scoped retrieval + audit; memory writes governed (`sera-rj4z`). | Cross-session retrieval test under `sera-e2e-harness` once `sera-rj4z` lands. | `sera-rj4z` (vector semantic memory + cross-session continuity). | IN_PROGRESS | P2 |
| 9 | Skills / procedure lifecycle | `sera-skills` crate covers discover / list / load. Profile-scoped availability and post-task suggestion partial. | Full discover / load / create / update / patch + profile-scoped availability + post-complex-task skill suggestion. **Differentiation:** skill writes are audited; skill changes are visible to HITL. | Smoke that loads a skill before a tool call and asserts the skill is recorded in audit; tracked under a new bead recommended below. | New bead recommended (working name: `sera-skills-parity`). | GAP | P3 |
| 10 | Delegation / subagents | Subagent spawn/list/status scaffolding in `sera-runtime`; OMC / Claude / Codex / Hermes profiles can be launched. Pattern A vs Pattern B distinction not yet enforced. | BYOH harnesses available with Pattern A audit when possible and Pattern B explicitly labeled opaque; subagent spawn becomes a GoalRun step rather than a loose helper call. **Differentiation:** GoalRun accountability, not chat-style spawn. | Spawn → status → close-out smoke under `sera-e2e-harness` with audit assertion; tracked under a new bead. | New bead recommended (working name: `sera-delegation-goalrun`). | GAP | P2 |
| 11 | Workflows / cron / webhooks | Workflow engine scaffolding in `sera-workflow`; cron/event/threshold/manual trigger work tracked in existing workflow beads. Webhook intake exists; sanitization layer (`sera-ctag`) not yet wired. | Cron / event / threshold / manual trigger + watchdog / no-agent script equivalent; webhook sanitization layer in front of prompt/context construction. **Differentiation:** workflow tasks are GoalRuns with stop conditions, evidence, and closeout. | Workflow trigger smoke + webhook sanitizer smoke under `sera-e2e-harness`. | `sera-ctag` (webhook sanitization), plus a new workflow-parity bead recommended for the GoalRun acceptance test. | GAP | P2 |
| 12 | TUI / dashboard / control plane | `sera-tui` (`crates/sera-tui`) + dashboard surfaces exist; control-plane completeness partial. | Sessions + approvals + agents + workflow / GoalRun state + logs + health + provider/budget status + audit drilldown + approvals queue. **Differentiation:** operator console for accountable work, not a generic chat UI. | Manual TUI smoke + a non-interactive control-plane smoke under `sera-e2e-harness`; tracked under a new bead recommended below. | New bead recommended (working name: `sera-control-plane-parity`). | GAP | P3 |
| 13 | Audit / HITL / policy | `sera-policy` scaffolding + audit log rows in SQLite. HITL approval surfaces partial; OCSF-shaped events not yet wired. | Immutable, OCSF-shaped audit ledger; HITL approval flows for write/execute/admin; CapabilityPolicy / PrincipalPolicy enforced before execution. **Differentiation:** this is the moat — do not trade it for short-term parity. | Append-only ledger smoke + HITL approval round-trip + policy-deny smoke under `sera-e2e-harness`. | `sera-q66q` (audit ledger), `sera-ctag` (sanitizer), `sera-tqzd` (failure visibility). New bead recommended for the HITL approval smoke. | IN_PROGRESS | P2 |

---

## Recommended new beads

Where this matrix has no canonical anchor, file beads with concrete acceptance
criteria rather than a single "parity epic". Suggested working names and
acceptance criteria (file the canonical bead before doing implementation
work):

1. **`sera-tool-parity-file`** — File / search / patch tool parity smoke,
   including mtime conflict and denied-write tests, plus audit assertion.
2. **`sera-tool-parity-terminal`** — Terminal / process tool parity smoke,
   including sandbox-deny + approval gate + timeout containment.
3. **`sera-mcp-parity`** — Web / browser / MCP bridge tool round-trip smoke
   with egress / SSRF guardrails.
4. **`sera-skills-parity`** — Skill load-before-tool-call smoke with audit.
5. **`sera-delegation-goalrun`** — Subagent spawn / status / close-out smoke
   expressed as a GoalRun step.
6. **`sera-workflow-parity`** — Cron / event / threshold trigger smoke +
   webhook sanitizer integration (joins on `sera-ctag`).
7. **`sera-control-plane-parity`** — TUI + dashboard control-plane smoke.
8. **`sera-hitl-parity`** — HITL approval round-trip smoke (joins on
   `sera-q66q`).
9. **`sera-baseline-auth-enforced`** — extend the Phase 0 baseline gate
   with an auth-enabled harness profile so Probe 2 enforces
   `Authorization: Bearer` and a paired negative request asserts 401
   without it. The current `hermes_parity_baseline_gate` already sends
   the bearer header and is forward-fit, but autonomous-mode boot
   (`InProcessGateway::start_local`) does not enforce auth, so the
   gate verifies HTTP shape + sanitizer + freshness only, not the
   auth-protected path.

These names are placeholders for the canonical beads; do not invent IDs in
this matrix. Each row above will be updated to point at the canonical bead
once filed.

---

## Update protocol

1. When a row's owning bead changes status, update the matrix row in the
   same PR (or follow-up PR) that changes the bead.
2. When a new row is needed (a Hermes capability we previously missed),
   add it under the same column shape; do not collapse it into an
   existing row.
3. Keep the matrix public-safe: redact private paths, secret names, and
   operator chat content. The baseline live-smoke report
   (`docs/internal/sessions/hermes-parity-baseline-template.md`) is the place for
   operator-run capture details — even there, do not paste secrets.
4. When a row reaches `BASELINE`, link it from the **Live baseline gate**
   table at the top so future readers can see at a glance which probes
   already cover it.

# Hermes → SERA Parity: GoalRun / BYOH workflow slice

> **Status:** Living document. First parity slice that turns a recurring
> Hermes-side worker loop into a SERA-native `WorkflowTask` (GoalRun) with
> bounded authority and observable evidence.
>
> **Parent:** `docs/internal/plans/hermes-parity-matrix.md` (rows 10 and 11).
>
> **Public-safe:** no secrets, no operator chat, no home-network paths.

## 1. Why this slice

The Hermes Kanban runtime drives recurring delegated operations — most
visibly the **PR closeout watcher** that bridges between a kanban task and
the GitHub pull request that ultimately closes it. Today the worker holds
that loop in its own context: it polls the PR, branches on the terminal
state, and dispatches follow-up work (repair, escalate, write the
bridge report). That is the exact shape of a Hermes "recurring /
delegated" operation that the parity matrix flags as a GAP for SERA-native
parity — see row 10 (delegation/subagents) and row 11
(workflows/cron/webhooks).

This slice picks one such operation — the PR closeout watcher — and
shows the smallest correct expression of it as a SERA-native GoalRun.
That gives us:

- A concrete migration path off the Hermes-side runtime loop.
- A bounded-authority surface (the gateway owns the gate, not the
  subagent).
- An audit-visible evidence trail (the delegation notice) that exists
  whether the subagent succeeds or fails.

All of these preserve the SERA differentiation axis: **work order +
bounded authority + evidence + audit / provenance**, rather than a
free-form chat-style loop.

## 2. Recurring Hermes operation selected

**PR closeout watcher**

A worker that:

1. Is given a PR id (and the repo).
2. Polls the PR until it reaches a terminal state.
3. Dispatches a BYOH closeout step on terminal — either the merge
   summariser, the repair branch, or the escalation handler — based on
   the terminal state.
4. Writes the closeout artefact (the bridge report) and exits.

The loop is recurring (one per delegated task), delegated (the closeout
step itself is a subagent dispatch), and externally gated (waits on a
GitHub state, not a SERA-internal signal). All three properties make it
the right first parity target.

## 3. SERA-native GoalRun envelope

The closeout watcher maps cleanly onto types that already exist in
`sera-workflow`:

| GoalRun envelope component | SERA-native expression |
|---|---|
| **Objective** | "Drive PR `<pr_id>` to a terminal closeout state and record the outcome." Encoded as the `WorkflowTask::title` plus a single acceptance criterion. |
| **Authority envelope** | `WorkflowTask` with `await_type: AwaitType::GhPr { pr_id, repo }`. The task is bounded to that single PR; it cannot wake on any other PR's state. The gateway owns the task row and the `GhPrLookup` that feeds it — the BYOH subagent does **not** decide its own gate. |
| **Fixture / agent lane** | The closeout step is a subagent dispatch via the `sera-runtime` delegate-task agent-tool. The dispatch is recorded as a `SubagentDelegationNotice { caller, target, tokens_used }` and forwarded to a `SubagentDelegationObserver` attached to the workflow `Coordinator`. |
| **Evidence** | Every dispatch emits exactly one `SubagentDelegationNotice` to the observer. The observer is the audit-ledger seam (see parity-matrix row 13, `sera-q66q`). No closeout step is invisible to audit. |
| **Stop conditions** | `GhPrState::is_terminal()` — `Closed` or `Merged`. Non-terminal states (`Open`, `Draft`, `Unknown`) deliberately keep the task blocked. The handler must wake on `Closed` too so a closed-without-merge PR does not strand the GoalRun. |
| **Evaluator** | `is_gh_pr_ready` (pure function over the gate + a `GhPrLookup` snapshot). When it returns `true`, the closeout handler branches on the concrete `GhPrState` to choose merge-summary vs repair vs escalate. |
| **Closeout** | The handler writes its bridge report, transitions the `WorkflowTask` to `WorkflowTaskStatus::Closed` (or `Blocked` on an unsafe ambiguity), and emits the final `SubagentDelegationNotice` for the closeout dispatch. The terminal `GhPrState` is recorded alongside the report. |

The **differentiation hooks** carried by this shape:

- Bounded authority: the BYOH subagent cannot widen the gate or shift it
  to another PR — it is given a pre-bound `pr_id` by the workflow row.
- Evidence per delegation: the audit ledger captures every closeout
  dispatch via the observer seam, even if the subagent itself crashes
  without writing a report.
- Pull-based gate: the scheduler polls `GhPrLookup` from the gateway
  side; the subagent does not own polling, which keeps rate-limit and
  egress policy in one place (parity-matrix row 7).

## 4. Smallest acceptance slice

The slice this doc ships:

1. This doc — maps one recurring Hermes operation to the GoalRun
   envelope. (You are reading it.)
2. A `sera-workflow` test
   (`pr_closeout_watcher_goalrun_emits_evidence_on_terminal`) that
   exercises the full envelope end-to-end against in-memory fixtures:
   - Constructs a `WorkflowTask` with `AwaitType::GhPr`.
   - Snapshots a `GhPrLookup` in three states (`Open`, `Closed`,
     `Merged`).
   - Drives the gate through `ready_tasks_with_context` and asserts the
     task is **not** ready while `Open`, **is** ready on `Merged`, and
     **is** ready on `Closed` (so closed-without-merge does not strand
     the GoalRun).
   - Wires a `Coordinator` with a `SubagentDelegationObserver` that
     records notices, then publishes the closeout dispatch as a
     `SubagentDelegationNotice`. Asserts the observer captures it
     (the audit-trail surrogate).
3. Parity-matrix update: rows 10 and 11 reference this doc as the first
   GoalRun BYOH parity anchor.

The test deliberately stays within `sera-workflow`: no HTTP, no real
GitHub, no real subagent process. The point is to prove the **shape** of
the migration path — that the types already on hand compose into a
GoalRun envelope — not to ship the full watchdog.

## 5. Out of scope (filed as follow-ups)

- HTTP wiring for non-Timer `await_type` values in the workflow router
  (see `scenarios_s7_workflow.rs::s7_2`). The matrix already tracks this
  as part of row 11.
- A real `GhPrLookup` backed by `octocrab` with rate-limit + egress
  policy. Tracked under the recommended `sera-mcp-parity` bead (egress
  guardrails) and a new follow-up for the polling source.
- Connecting the `SubagentDelegationObserver` to the OCSF audit ledger
  shipped in `sera-tqzd` / `sera-q66q`. The seam exists; the wiring is
  the next slice.

## 6. References

- `docs/internal/plans/hermes-parity-matrix.md` — rows 10 and 11.
- `rust/crates/sera-workflow/src/task.rs` — `WorkflowTask`,
  `AwaitType::GhPr`, `GhPrState::is_terminal`.
- `rust/crates/sera-workflow/src/ready.rs` — `is_gh_pr_ready`,
  `GhPrLookup`, `ready_tasks_with_context`.
- `rust/crates/sera-workflow/src/coordination.rs` —
  `SubagentDelegationNotice`, `SubagentDelegationObserver`,
  `Coordinator::with_subagent_observer`,
  `Coordinator::publish_subagent_notice`.
- `rust/crates/sera-workflow/src/tests.rs` —
  `pr_closeout_watcher_goalrun_emits_evidence_on_terminal`.

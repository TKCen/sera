# ADR — Workflow Task Storage Split

> **Status:** ACCEPTED — 2026-05-02
> **Bead:** [`sera-dhvh`](../../../). Anchor for the split.
> **Authors:** SERA architecture lane.
> **Scope:** documentation + doc-comment cross-references on the two existing types. **No** runtime/storage migration in this PR.

---

## 1. Context

A 2026-04-29 duplicate-implementation audit flagged that two crates ship rusqlite-backed stores keyed on `sera_workflow::task::WorkflowTask`:

- **`sera-workflow::engine::WorkflowEngine`** (PR #979 — re-exported via `sera_workflow::WorkflowEngine`). Trait `WorkflowEngineBackend` with `SqliteWorkflowBackend` + `MemoryWorkflowBackend`. Operations: `submit_task`, `claim_next_ready` (atomic CAS `Open → Hooked`), `mark_complete`, `mark_failed`, `recover_orphans`, `load`. Schema columns: `id`, `status`, `priority`, `payload`, `defer_until`, `claimed_at`, `claimed_by`, `result`, `failure_reason`. Status surface is the full beads issue lifecycle: `Open | InProgress | Hooked | Blocked | Deferred | Closed | Pinned`.

- **`sera-gateway::workflow_store::WorkflowTaskStore`** (PR #1159 — `sera-d2xh` persistence on top of the in-memory `sera-kgi8` store). Trait `WorkflowTaskStore` with `InMemoryWorkflowTaskStore` + `SqliteWorkflowTaskStore`. Operations: `insert`, `get`, `list`, `list_pending`, `mark_resolved`. Wraps each task in a `WorkflowTaskRecord { task, agent_id, resume_token, status: SchedulerTaskStatus, resolved_at }`. Schema columns: `id`, `agent_id`, `resume_token`, `status`, `resolved_at`, `task_json`, `created_at`. Status surface is a binary `Pending | Resolved`.

Both reuse `sera_workflow::task::WorkflowTask` as the payload, both pick rusqlite, and both ship `In-Memory + SQLite` impls behind a trait. Phase 1 of the gateway scheduler (`SPEC-gateway` §scheduler) is explicit that persistence is a follow-up — and now that follow-up has landed alongside the engine. The bead asks: are these two implementations of one concept, or two separate concepts wearing similar clothes?

The split matters because it shows up at every call site in `rust/crates/sera-gateway/src/bin/sera.rs` (13+ uses of `InMemoryWorkflowTaskStore::new()`), in `routes/workflow.rs` and `admin/state.rs` (trait-object plumbing), and in the scheduler tick loop (`scheduler::tick`). Picking the wrong answer means either a permanent two-store regime or a ~150-line refactor that touches the gateway boot path.

## 2. Decision

**Keep the two stores separate. They are not duplicates — they model two distinct lifecycles over the same task payload. Document the boundary in this ADR and rename only via doc-comments.**

Concretely:

| Concern | Owner | Lifecycle |
|---|---|---|
| **Claim / dispatch** — "which `WorkflowTask` does the next agent pull?" | `sera-workflow::WorkflowEngine` | `Open → Hooked → InProgress → Closed` (or `Blocked` on failure) — full beads issue lifecycle. |
| **Gate / resume** — "which suspended runtime continuation has its external `AwaitType` satisfied?" | `sera-gateway::workflow_store::WorkflowTaskStore` | `Pending → Resolved` — binary scheduler-side resume signal. |

The two lifecycles are orthogonal. A single `WorkflowTask` can in principle traverse both: an agent claims it (engine: `Open → Hooked`), the runtime suspends on `AwaitType::GhRun` (scheduler: `Pending`), the GhRun terminates (scheduler: `Resolved` → wake event), the runtime resumes and finally marks the task closed (engine: `Hooked → Closed`). The engine answers "who owns this task?" and the scheduler answers "is its external gate ready?". A single store would have to fuse two orthogonal status enums and two orthogonal write paths into one schema.

This ADR records that boundary, names each store's distinct purpose, and leaves the existing code untouched.

## 3. Why not consolidate

Consolidation is the tempting option (one durable store, no overlap). It is rejected for five reasons:

1. **Different status surfaces.** Engine status is the 7-variant beads-issue enum. Scheduler status is `Pending | Resolved`. Collapsing them either drops information (lose the binary gate signal) or bloats the engine status with two extra variants that only the scheduler reads. Neither is clean.
2. **Different schema-relevant fields.** Scheduler needs `agent_id` and `resume_token` columns to route the wake event back to the originating runtime continuation — these have no engine analogue (the engine's `claimed_by` is a worker label, not a routing target). Engine needs `priority`, `defer_until`, `claimed_at` indexed for the atomic-CAS query — these have no scheduler analogue (the scheduler reads every pending row each tick).
3. **Different write cadences.** Engine writes are agent-driven (claim, complete, fail) — read-mostly with rare contention. Scheduler writes are tick-driven every 5 s, plus per-gate-event ingress writes. The engine's `UPDATE … RETURNING` claim CAS and the scheduler's `UPDATE … WHERE status = 'pending'` mark-resolved are independently optimisable; merging them couples two unrelated write profiles.
4. **`WorkflowEngine` has zero call sites outside its own crate today.** Grep `WorkflowEngine\|SqliteWorkflowBackend` across the workspace: every hit is in `sera-workflow/src/engine.rs` or `sera-workflow/tests/`. The engine is scaffolded ahead of the gateway claim-dispatch integration (which will land alongside `sera-y45a` embedded dispatch). Folding it into the gateway store now would force a premature shape on code that has not been wired up to a caller. The stores can be merged later from a position of evidence; merging now is speculative.
5. **The boundary is load-bearing for the dispatch ADR.** [`2026-04-29-dispatch-ownership.md`](2026-04-29-dispatch-ownership.md) commits to gateway-owned dispatch, with `sera-runtime` ultimately running embedded-library inside the gateway process. Once that lands, the engine's `claim_next_ready` and the scheduler's `list_pending` run *in the same process* and *over the same SQLite file* — but they remain two tables answering two questions. Consolidating prematurely would invert the dispatch ADR's "thin embedded library" framing into a "the engine *is* the scheduler" framing the spec does not endorse.

## 4. Boundary contract

Code that touches workflow persistence must respect the following invariants. They are written here so future contributors do not have to reverse-engineer them from the two implementations.

### 4.1 What `sera-workflow::WorkflowEngine` owns

- The **claim protocol** — `submit_task`, `claim_next_ready`, `mark_complete`, `mark_failed`, `recover_orphans`. Anything that mutates `WorkflowTaskStatus` (the beads-issue lifecycle).
- The **`Hooked`-state heartbeat / orphan-reaper** for stale claims (`recover_orphans`).
- The **canonical task payload row.** When both stores are wired together (post-`sera-y45a`), the engine's `payload` column is the source of truth for task content; the scheduler's `task_json` is a routing snapshot taken at suspension time.

### 4.2 What `sera-gateway::workflow_store::WorkflowTaskStore` owns

- The **gate-suspension table** — every runtime continuation that hit `runtime.suspend(workflow_task)` and is now waiting for an `AwaitType` gate (`Timer | Mail | GhRun | GhPr | Human | Change`).
- The **`agent_id` + `resume_token` routing pair** — opaque correlation handles the runtime supplied at suspension time, returned verbatim on the wake event.
- The **`Pending → Resolved` transition** — driven by `scheduler::tick` reading `list_pending` every 5 s and consulting the per-gate lookups.

### 4.3 What neither owns (today)

- **Cross-table consistency.** A task can be `Closed` in the engine while `Pending` in the scheduler if the runtime suspended on a gate, the agent then closed the issue out-of-band, and the gate has not yet resolved. Resolving this is a follow-up reconciler bead, not a storage-layer concern. (See §5 follow-ups.)
- **A single SQLite file.** Each crate opens its own `Connection`. Co-locating them in one DB file (separate tables) is allowed but not required; if they ever do co-locate, the gateway boot path opens both via shared connection-config.

### 4.4 Naming convention

To make the boundary readable, doc-comments on both types now point at this ADR and use the names **"workflow engine store"** (claim/dispatch) and **"workflow scheduler store"** (gate/resume). The Rust type names are unchanged — renaming them would touch every gateway boot site for cosmetic gain.

## 5. Follow-ups (not blocking this PR)

| Bead | What it lands |
|---|---|
| _(file later)_ | Wire `WorkflowEngine::claim_next_ready` into the gateway dispatch path once the dispatch-ownership ADR's step 2 (embedded mode) lands. Today the engine has no caller; that bead gives it one. |
| _(file later)_ | Cross-store reconciler: when the engine flips a task to `Closed` or `Blocked`, mark the matching scheduler row `Resolved` (terminal) so a stranded gate doesn't hold the row in `Pending` forever. Detection is a join on `WorkflowTaskId`; reconciliation runs as a periodic gateway maintenance task. |
| _(file later)_ | Optional consolidation pass — once both stores have real callers, re-evaluate whether merging into `workflow.sqlite` (two tables, one connection) is worth the refactor. This ADR explicitly does **not** commit to that merge; the case for it has to be made on integration evidence. |

## 6. Consequences

- **`sera-d2xh` (PR #1159) and `sera-y45a` (engine + dispatch) stay independent.** Neither bead is gated on the other. Each can ship its own E2E.
- **The duplicate-implementation audit's question is answered.** The two stores are intentionally separate; future audits should not flag them as duplicates without first reading this ADR.
- **No code change in this PR beyond doc-comments.** Specifically: zero changes to `bin/sera.rs`, `scheduler.rs`, `routes/workflow.rs`, `admin/state.rs`, the engine's SQL schema, or the workflow-store's SQL schema.
- **`SPEC-workflow-engine` and `SPEC-gateway` continue to name their respective stores as the canonical owner** of the lifecycle they describe. No spec rewrite needed.
- **If the dispatch-ownership ADR step 2 (embedded mode) opts to share a SQLite file** between the engine and the scheduler, that is an implementation detail and does not violate this ADR — they remain two tables.

## 7. Alternatives considered

- **Option A — consolidate into `sera-workflow::WorkflowEngine`, drop the gateway's `WorkflowTaskStore`.** Rejected. Forces the engine's status enum to absorb `Pending | Resolved` (or invent a parallel `gate_status` column), forces the engine to carry `agent_id` + `resume_token` for a routing concern it does not own today, and requires a 13-call-site refactor in `bin/sera.rs` for a hypothetical future caller of `claim_next_ready`. Negative-value trade today.
- **Option B — consolidate into `sera-gateway::workflow_store::WorkflowTaskStore`, drop `WorkflowEngine`.** Rejected. The engine is the canonical surface in `SPEC-workflow-engine §4a–§4d` (claim, mark_complete/failed, recover_orphans). Inverting the spec to delete it would also delete the orphan-reaper and the atomic CAS — both of which the gateway will need once `sera-y45a` step 2 wires dispatch through.
- **Option D — separate but enforce a single shared connection.** Rejected as premature. Each crate picks its connection independently; co-location in a single file is a follow-up implementation detail, not an architectural commitment.

## 8. References

- Bead [`sera-dhvh`](../../../) — this ADR's anchor.
- `rust/crates/sera-workflow/src/engine.rs` — `WorkflowEngine` + `SqliteWorkflowBackend` + `MemoryWorkflowBackend`.
- `rust/crates/sera-workflow/src/lib.rs` — re-exports `EngineError`, `MemoryWorkflowBackend`, `SqliteWorkflowBackend`, `WorkflowEngine`, `WorkflowEngineBackend`.
- `rust/crates/sera-gateway/src/workflow_store.rs` — `WorkflowTaskStore` + `InMemoryWorkflowTaskStore` + `SqliteWorkflowTaskStore`.
- `rust/crates/sera-gateway/src/scheduler.rs` — gate-resume tick loop using `WorkflowTaskStore::list_pending` + `mark_resolved`.
- [`2026-04-29-dispatch-ownership.md`](2026-04-29-dispatch-ownership.md) — the dispatch-ownership ADR; this ADR keeps the boundary the dispatch ADR's embedded-mode step relies on.
- `docs/internal/plans/HANDOFF.md` §"Architectural decisions baked this session" — "Gateway is the only durable-state owner" and "Local-first default. SQLite + files." This ADR honours both.

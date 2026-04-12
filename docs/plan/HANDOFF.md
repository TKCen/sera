# SERA 2.0 Phase 0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** audit round → `git show HEAD~1:docs/plan/HANDOFF.md`; spec round → `git show HEAD~2:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**Single P0 task from the previous handoff: completed.** Produced [`docs/plan/PHASE-0-PLAN.md`](PHASE-0-PLAN.md) — 3248 lines, code-level breakdown covering all 9 P0 items from `IMPL-AUDIT.md` §5, with sequencing graph, parallel lanes A–F, milestone exit criteria M0–M4, and a §4.6 design-forward-obligations traceability matrix.

Produced via orchestrated fan-out: 5 Wave 1 sonnet agents (one per crate group) + 2 Wave 2 sonnet agents (sequencing + test catalog) + 1 Haiku stager, assembled via bash concatenation to keep main context lean. No Rust code was written this session — the plan is a document, implementation starts next session.

---

## 2. The Phase 0 plan document — how to use it

[`docs/plan/PHASE-0-PLAN.md`](PHASE-0-PLAN.md) is structured as:

1. **Preamble + how-to-use + document map** — one-page orientation.
2. **§Sequencing, parallel lanes, and milestones** — inter-crate ordering, lane assignments (A–F), M0–M4 exit criteria. **Read this first.**
3. **Nine P0-N sections** (`P0-1 sera-types` through `P0-9 sera-workflow` plus `P0-10 partial scaffolding`) — each self-contained: strategy, files to create/modify, Cargo features, workspace deps, acceptance tests, downstream cascade. Agents working on one crate need only their section.
4. **§Acceptance-test catalog** — maps every IMPL-AUDIT §4.6 obligation to a test function. Any row marked `GAP` blocks M4.

**Read order for next session:** §Sequencing → §P0-1 (the single first-mover) → drill into other P0-N sections only when starting that lane.

---

## 3. What's next — Lane A (serialised, blocks everything)

Per `PHASE-0-PLAN.md` §Sequencing, Lane A runs alone as the sole first-mover. **No parallel lanes may begin until M0 is reached.**

**Lane A = P0-1 · `sera-domain` → `sera-types` rename + design-forward primitives.**

Two commits on a single branch:

1. **Commit 1: mechanical rename.** `git mv crates/sera-domain crates/sera-types`; update workspace `Cargo.toml` members + workspace-dependencies key; patch 13 dependent `Cargo.toml` files from `sera-domain.workspace = true` to `sera-types.workspace = true`; global search-replace `use sera_domain::` → `use sera_types::`. Gate: `cargo check --workspace` shows only expected downstream structural breakage (not rename-caused errors).

2. **Commit 2: design-forward primitives.** Ultrawork-parallelisable — create `src/evolution.rs`, `src/versioning.rs`, `src/content_block.rs` and patch `src/runtime.rs`, `src/session.rs`, `src/config_manifest.rs`, `src/capability.rs`, `src/hook.rs`, `src/lib.rs` independently. 15 acceptance tests across `tests/evolution.rs`, `tests/versioning.rs`, `tests/content_block.rs`, `tests/session.rs`, `tests/config_manifest.rs`.

M0 exit criteria (copy from PHASE-0-PLAN.md §M0):
- `cargo check -p sera-types` green on default features.
- All 15 P0-1 acceptance tests pass.
- Downstream breakage triaged with `// TODO(P0-5/P0-6)` stubs at `sera-runtime` `TurnResult` match sites, `sera-hooks` `HookPoint::ALL` count assertion (16 → 20), `sera-core` gateway pipeline pattern-match.

**After M0 lands**, the next session can fan out four lanes in parallel: Lane B (telemetry + config + queue), Lane C (tools + scaffolds), Lane E (workflow + auth split to two agents), and begin Lane D's rename commit.

---

## 4. Known gaps in the plan (must fix before M4)

The §4.6 traceability matrix flagged four obligations without a matching acceptance test. The implementer must add these during Lane A (items §4, §11, §12, §13 all live in sera-types):

- **§4 `ConstitutionalRule`** — add `constitutional_rule_serde_roundtrip` to `tests/evolution.rs`. Verify all 4 fields plus `ConstitutionalEnforcementPoint` exhaustive variants.
- **§11 `HookPoint::ConstitutionalGate` fail-closed** — add `hook_point_constitutional_gate_is_fail_closed` to a new `tests/hooks.rs`. Assert `HookPoint::ALL.len() == 20` and that the gate's default enforcement is fail-closed.
- **§12 `HookContext.change_artifact`** — add `hook_context_change_artifact_field_roundtrip` to `tests/hooks.rs`. Round-trip a `HookContext { change_artifact: Some(ChangeArtifactId([1u8;32])), .. }`.
- **§13 `HookResult::updated_input`** — add `hook_result_updated_input_roundtrip` to `tests/hooks.rs`. Round-trip `HookResult::Continue { updated_input: Some(json!(...)) }`.

These are tracked in PHASE-0-PLAN.md's §Acceptance-test catalog → Gaps subsection.

---

## 5. Gotchas carried forward

Unchanged from previous handoffs — all still apply:

- **§6.1 Hook false-alarms** on Edit/Write. Tool confirmation is authoritative; verify with `ls` if uncertain.
- **§6.2 OTel triad version lock** is load-bearing. `opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"` MUST be exact-equals. Drift → compile-time trait bound errors.
- **§6.3 `wasmtime`** pinned loose `">=43, <50"`. Revisit quarterly.
- **§6.4 Beads is Go, not Rust.** SERA integrates beads by shelling out to `bd` CLI in Phase 1, not as a Rust library dep. sera-workflow's `WorkflowTask` mirrors the beads `Issue` schema but does not depend on it.
- **§6.5 Context window pressure.** Previous session hit 73%; this session stayed leaner by extracting Wave 1 outputs to `/tmp/phase0-plan/*.md` and concatenating via bash. Next session should delete `/tmp/phase0-plan/` only after confirming `PHASE-0-PLAN.md` reads clean (the tmp files are redundant with the committed plan).
- **§6.6 Rename-in-place** is the agreed strategy for `sera-domain` → `sera-types`. `git mv`, update 13 Cargo.toml files, global `use` search-replace. See PHASE-0-PLAN.md §P0-1 for the full Cargo.toml edit table.
- **§6.7 Hook stop-loop on cancel.** `/oh-my-claudecode:cancel` clears ultrawork + skill-active state; if the stop hook keeps firing, re-run cancel.

---

## 6. Files that exist and matter

- **`docs/plan/HANDOFF.md`** — this file
- **`docs/plan/PHASE-0-PLAN.md`** — the new 3248-line Phase 0 plan
- **`docs/plan/IMPL-AUDIT.md`** — the audit (869 lines); still the authority for "what should exist"
- **19 patched specs** in `docs/plan/specs/`
- **`docs/plan/plan.md`** and **`docs/plan/architecture.md`** — unchanged, still authoritative

Do not re-read `plan.md`, `architecture.md`, or whole specs cold. PHASE-0-PLAN.md and IMPL-AUDIT.md's per-crate deltas already cite the specific sections that matter.

---

## 7. Cross-reference map (carried forward)

Unchanged from previous handoff. When a future change touches one of these concerns, look at every listed spec to maintain consistency:

| Concern | Specs that matter |
|---|---|
| New external crate added | SPEC-dependencies §5–§9, SPEC-crate-decomposition §3, SPEC-versioning §5 |
| Gateway↔Harness transport change | SPEC-gateway §3 §7a, SPEC-runtime §2.2, SPEC-dependencies §10.2 |
| Hook point added/removed | SPEC-hooks §3, SPEC-runtime §10, SPEC-gateway §3.2, SPEC-self-evolution §5.3 |
| Approval scope added | SPEC-hitl-approval §2 §3 §5a, SPEC-self-evolution §9, SPEC-identity-authz §5.1a |
| Sandbox policy change | SPEC-tools §6a, SPEC-security §4, SPEC-dependencies §10.8 §10.18, SPEC-secrets §5a |
| Multi-agent coordination | SPEC-circles §3 §5, SPEC-workflow-engine §4, SPEC-runtime §9a |
| Memory tier change | SPEC-memory §2.0, SPEC-runtime §6a, SPEC-dependencies §10.16 |
| Self-evolution scope added | SPEC-self-evolution §9, SPEC-hitl-approval §5e, SPEC-identity-authz §5.1b, SPEC-config §7a §7b |
| Audit event added | SPEC-observability §3.0 §3.1, SPEC-self-evolution §5.7, SPEC-security §4.6 |
| Crate audit / delta question | `docs/plan/IMPL-AUDIT.md` §2 — per-crate sections |
| **Phase 0 implementation question (NEW)** | **`docs/plan/PHASE-0-PLAN.md` §P0-N — per-item sections** |

---

**End of handoff.** A fresh session reading this file plus `PHASE-0-PLAN.md` §Sequencing and §P0-1 has everything needed to start Lane A (M0).

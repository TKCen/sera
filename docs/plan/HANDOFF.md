# SERA 2.0 Phase 0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** plan round → `git show 216c32c:docs/plan/HANDOFF.md`; audit round → `git show 216c32c~1:docs/plan/HANDOFF.md`; spec round → `git show 216c32c~2:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**M0 milestone reached.** Lane A (P0-1) is complete — `sera-domain` renamed to `sera-types`, design-forward primitives added, 19 acceptance tests passing, full workspace compiles clean.

Three commits on `sera20`:

1. **`37ca870` — refactor: rename sera-domain to sera-types.** `git mv`, 13 dependent Cargo.toml patches, global `sera_domain` → `sera_types` search-replace. `cargo check --workspace` clean.

2. **`e49d07f` — feat: design-forward primitives.** New files: `evolution.rs` (ChangeArtifactId, BlastRadius 22 variants, CapabilityToken, ConstitutionalRule, EvolutionTier, AgentCapability), `versioning.rs` (BuildIdentity), `content_block.rs` (ContentBlock, ConversationRole, ConversationMessage, ActionId). Patched: `runtime.rs` (+TurnOutcome 6-variant enum, TurnContext.change_artifact), `session.rs` (+5 SessionState variants with transition arcs), `config_manifest.rs` (+3 ResourceKind variants, ResourceMetadata.shadow, PersonaSpec.mutable_persona/mutable_token_budget), `capability.rs` (+AgentCapability enum), `hook.rs` (+4 HookPoint variants → 20 total, HookContext.change_artifact, HookResult::Continue.updated_input). `#[non_exhaustive]` on ResourceKind, SessionState, BlastRadius, EvolutionTier. 19 acceptance tests (15 catalog + 4 GAP).

3. **`4fac0d9` — chore: delete old sera-domain, add Phase 0 plan docs.**

Downstream breakage triaged with `// TODO(P0-5/P0-6)` stubs:
- `sera-hooks/src/executor.rs:109` — `..` pattern for `updated_input` field
- `sera-core/src/bin/sera.rs` — `change_artifact: None` at 4 HookContext construction sites

---

## 2. What's next — four parallel lanes (M0 is reached)

Per `PHASE-0-PLAN.md` §Sequencing, M0 unblocks four parallel lanes. The next session should fan out immediately.

### Lane B — infrastructure foundations (3 parallel agents)

| Agent | P0 item | Key deliverable | PHASE-0-PLAN.md section |
|-------|---------|-----------------|------------------------|
| B1 | P0-2 sera-telemetry | New crate alongside sera-events; OTel triad pins; AuditBackend trait; LaneFailureClass 15 variants | §P0-2 |
| B2 | P0-3 sera-config | figment/schemars extension; ShadowConfigStore; ConfigVersionLog | §P0-3 |
| B3 | P0-4 sera-queue | Extract from sera-db; QueueBackend trait; LocalQueueBackend; GlobalThrottle; apalis feature gate | §P0-4 |

**Blocks:** Lane D (gateway needs QueueBackend stable)

### Lane C — tools absorption (1–2 agents)

| Agent | P0 item | Key deliverable | PHASE-0-PLAN.md section |
|-------|---------|-----------------|------------------------|
| C1 | P0-8 sera-tools | New crate absorbing sera-docker; SandboxProvider trait; SsrfValidator; CON-04 kill-switch | §P0-8 |
| C2 | P0-10 partial | Scaffold sera-errors, sera-cache, sera-secrets (leaf crates, no production logic) | §P0-10 |

**Blocks:** Lane D (gateway acquires DockerSandboxProvider)

### Lane D — gateway + runtime spine (1 agent, after B+C)

| Agent | P0 items | Key deliverable | PHASE-0-PLAN.md section |
|-------|----------|-----------------|------------------------|
| D1 | P0-5 + P0-6 | sera-core → sera-gateway rename; SQ/EQ envelope; AppServerTransport; TurnOutcome migration; four-method turn lifecycle; main.rs rewrite | §P0-5, §P0-6 |

**Must be single agent** — AgentHarness/AppServerTransport/main.rs form a three-way contract.

### Lane E — workflow + auth typing (2 agents, independent of B/C/D)

| Agent | P0 item | Key deliverable | PHASE-0-PLAN.md section |
|-------|---------|-----------------|------------------------|
| E1 | P0-9 sera-workflow | WorkflowTask (beads schema); WorkflowTaskId content-hash; atomic claim; termination triad | §P0-9 |
| E2 | P0-7 sera-auth | argon2 key hashing; casbin RBAC; CapabilityToken narrowing; Action::ProposeChange/ApproveChange | §P0-7 |

**Blocks:** Nothing in Phase 0

### Recommended orchestration

1. Fan out **5 agents** immediately: B1, B2, B3, C1, E1 (or E2). These are all independent after M0.
2. C2 (scaffolding) can run in parallel with C1 or as a quick follow-up.
3. E2 can run in parallel with E1.
4. **Lane D waits for Lanes B and C** (M1 milestone). Start D only after `cargo check -p sera-queue` and `cargo check -p sera-tools` are green.
5. Lane F (sera-testing, sera-session scaffolds) waits for Lane D.

### Milestone targets

- **M1** — Lanes B + C complete. `cargo check` green for sera-telemetry, sera-config, sera-queue, sera-tools, sera-errors, sera-cache, sera-secrets.
- **M2** — Lane D complete. Gateway + runtime spine wired. sera-docker shim deleted.
- **M3** — Lane E complete. Workflow + auth typed and tested.
- **M4** — All lanes + Lane F. `cargo check --workspace` clean across all feature matrix combos. Phase 0 done.

---

## 3. M0 verification checklist (confirmed)

All items below were verified before push:

- [x] `cargo check -p sera-types` passes (0 errors)
- [x] `cargo check --workspace` passes (0 errors, 14 crates)
- [x] `rust/crates/sera-types/` exists; `rust/crates/sera-domain/` deleted
- [x] All 13 dependent Cargo.toml files updated to `sera-types`
- [x] Zero `sera_domain` references in .rs files
- [x] Zero `sera-domain` references in Cargo.toml files
- [x] 19 acceptance tests pass (272 unit + 22 integration = 294 total)
- [x] Downstream breakage triaged with TODO stubs
- [x] 4 GAP tests from §4.6 traceability matrix implemented

---

## 4. Known gaps resolved

All four §4.6 traceability gaps from the previous handoff are now closed:

- **§4 `ConstitutionalRule`** — `constitutional_rule_serde_roundtrip` in `tests/evolution.rs`
- **§11 `HookPoint::ConstitutionalGate`** — `hook_point_constitutional_gate_is_fail_closed` in `tests/hooks.rs`
- **§12 `HookContext.change_artifact`** — `hook_context_change_artifact_field_roundtrip` in `tests/hooks.rs`
- **§13 `HookResult::updated_input`** — `hook_result_updated_input_roundtrip` in `tests/hooks.rs`

No remaining GAP items for sera-types. Future GAP items (if any) will surface in per-crate P0-N sections.

---

## 5. Design decisions made this session

- **TurnResult kept alongside TurnOutcome.** The plan called for "replace", but removing TurnResult would break sera-runtime's DefaultRuntime and sera-core's gateway pipeline (Lane D work). Both types coexist until P0-5/P0-6 migrates all consumers to TurnOutcome.
- **`[u8; 64]` serde for CapabilityToken.signature.** Serde doesn't auto-derive arrays > 32 bytes. Agent 1 added a custom `bytes64` serde helper module in `evolution.rs`.
- **HookResult::Continue `updated_input` field.** Added with `#[serde(skip_serializing_if = "Option::is_none")]` and defaulting to `None` in `pass()` / `pass_with()` helpers. Downstream `sera-hooks/executor.rs` uses `..` pattern to ignore it until P0-5/P0-6.

---

## 6. Gotchas carried forward

- **§6.1 Hook false-alarms** on Edit/Write. Tool confirmation is authoritative.
- **§6.2 OTel triad version lock** is load-bearing. `opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"` MUST be exact-equals.
- **§6.3 `wasmtime`** pinned loose `">=43, <50"`. Revisit quarterly.
- **§6.4 Beads is Go, not Rust.** Shell out to `bd` CLI in Phase 1.
- **§6.5 Context window pressure.** M0 session used ultrawork with 3 parallel sonnet agents + haiku finisher. Stayed under 60% context budget. Recommend same pattern for M1 fan-out.
- **§6.6 Rename-in-place** — DONE for sera-types. Still pending: `sera-core → sera-gateway` (P0-5), `sera-events → sera-telemetry` (P0-2, new-crate-alongside strategy).
- **§6.7 Hook stop-loop on cancel.** `/oh-my-claudecode:cancel` clears ultrawork + skill-active state; if the stop hook keeps firing, re-run cancel.
- **§6.8 `thiserror` v2 `source` fields** — any field named `source` is auto-treated as `#[source]`. Use `reason` for plain String error context.

---

## 7. Files that exist and matter

- **`docs/plan/HANDOFF.md`** — this file
- **`docs/plan/PHASE-0-PLAN.md`** — 3248-line Phase 0 plan (authority for all P0-N work)
- **`docs/plan/IMPL-AUDIT.md`** — the audit (869 lines; authority for "what should exist")
- **19 patched specs** in `docs/plan/specs/`
- **`rust/CLAUDE.md`** — Rust workspace dev guide (crate map needs updating for sera-types)

Do not re-read `plan.md`, `architecture.md`, or whole specs cold. PHASE-0-PLAN.md §P0-N sections cite the specific spec sections that matter.

---

## 8. Cross-reference map (carried forward)

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
| Phase 0 implementation question | `docs/plan/PHASE-0-PLAN.md` §P0-N — per-item sections |

---

**End of handoff.** A fresh session reading this file can immediately fan out Lanes B, C, and E. Lane D waits for M1.

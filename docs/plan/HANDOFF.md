# SERA 2.0 Phase 0 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-12
> **Previous handoffs:** M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; audit round → `git show 216c32c~1:docs/plan/HANDOFF.md`; spec round → `git show 216c32c~2:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. What this session accomplished

**M1 milestone reached. M3 milestone reached.** Lanes B, C, and E are complete. Seven new crates created, two extended, all compiling clean with full acceptance test suites.

Four commits on `sera20`:

1. **`bf10d5c` — feat: Lane B infrastructure crates.** sera-telemetry (new, 18 tests), sera-config extensions (14 new tests), sera-queue (new, 12 tests). OTel triad exact-pinned. AuditBackend OnceCell set-once. QueueBackend object-safe. ConfigVersionLog SHA-256 hash chain. ShadowConfigStore overlay.

2. **`edfd593` — feat: Lane C tools + P0-10 scaffolds.** sera-tools (new, 15 tests) with SandboxProvider trait, SsrfValidator, BinaryIdentity TOFU, BashAstChecker, KillSwitch CON-04. sera-errors, sera-cache, sera-secrets scaffolded.

3. **`65373d2` — feat: Lane E workflow + auth.** sera-workflow rewrite (14 new tests) with WorkflowTaskId content-hash, atomic claim protocol, ready_tasks() five-gate, termination triad. sera-auth extensions (12 new tests) with CapabilityToken narrowing, argon2 key hashing, casbin RBAC, Action::ProposeChange/ApproveChange.

4. **`ab42add` — chore: workspace wiring.** All new crates in workspace members/deps. OTel triad pins with load-bearing comments. Fixed sera-runtime TurnContext.change_artifact field.

---

## 2. Milestone verification

### M1 — infrastructure in place (confirmed)

- [x] `cargo check -p sera-telemetry` green; OTel triad pins present; AuditBackend object-safe; LaneFailureClass 15 variants
- [x] `cargo check -p sera-config` green; all design-forward config fields present
- [x] `cargo check -p sera-queue` green; QueueBackend object-safe; LocalQueueBackend roundtrip; GlobalThrottle; 12 tests pass
- [x] `cargo check -p sera-tools` green; SandboxProvider object-safe; SsrfValidator blocks loopback/link-local/metadata; CON-04 boot check; 15 tests pass
- [x] P0-10 scaffolds (sera-errors, sera-cache, sera-secrets) in workspace and compiling
- [x] `cargo check --workspace` green (all 21 crates)
- [x] `cargo test --workspace` — 0 failures

### M3 — workflow and auth typed (confirmed)

- [x] `cargo check -p sera-workflow` green; WorkflowTaskId is [u8;32] content hash; WorkflowTaskStatus::Hooked present; 14 new tests pass
- [x] ready_tasks() five-gate algorithm passes all readiness tests including ConditionalBlocks
- [x] claim_task() CAS passes atomic_claim and double_claim tests
- [x] WorkflowTask.meta_scope and .change_artifact_id fields present and serde-stable
- [x] `cargo check -p sera-auth` green on default and --no-default-features
- [x] StoredApiKey.key_hash_argon2 PHC string; no plaintext comparison path
- [x] CapabilityToken::narrow() widening rejection tested; proposal limit tested
- [x] CasbinAuthzAdapter wired; RBAC allow/deny tests pass
- [x] Action::ProposeChange/ApproveChange and Resource::ChangeArtifact present
- [x] 12 auth acceptance tests pass

---

## 3. What's next — Lane D (gateway + runtime spine)

Per `PHASE-0-PLAN.md` §Sequencing, M1 + M3 unblock Lane D.

### Lane D — gateway and runtime spine (1 agent, after B+C)

| Agent | P0 items | Key deliverable | PHASE-0-PLAN.md section |
|-------|----------|-----------------|------------------------|
| D1 | P0-5 + P0-6 | sera-core → sera-gateway rename; SQ/EQ envelope; AppServerTransport; TurnOutcome migration; four-method turn lifecycle; main.rs rewrite | §P0-5, §P0-6 |

**Must be single agent** — AgentHarness/AppServerTransport/main.rs form a three-way contract.

### Lane F — scaffolding completion (after D)

| Agent | P0 items | Key deliverable |
|-------|----------|-----------------|
| F1 | P0-10 remainder | sera-testing (mock QueueBackend + mock SandboxProvider), sera-session (6-state machine) |

### Recommended orchestration

1. **Start Lane D immediately** — single opus agent for P0-5 + P0-6. Rename commit first, then SQ/EQ structural additions, then runtime contract migration.
2. After Lane D, fire Lane F for sera-testing and sera-session scaffolds.
3. sera-docker shim deletion is part of P0-5/P0-6 (call-site migration).

### Milestone targets

- **M2** — Lane D complete. Gateway + runtime spine wired. sera-docker shim deleted.
- **M4** — All lanes + Lane F. `cargo check --workspace` clean across all feature matrix combos. Phase 0 done.

---

## 4. Crate inventory (21 workspace members)

| Crate | Status | Tests | Lane |
|-------|--------|-------|------|
| sera-types | M0 stable | 272 unit + 22 integration | A |
| sera-telemetry | **NEW** M1 | 18 | B |
| sera-config | Extended M1 | 66 (14 new) | B |
| sera-queue | **NEW** M1 | 12 | B |
| sera-tools | **NEW** M1 | 15 | C |
| sera-errors | **NEW** scaffold | 0 | C |
| sera-cache | **NEW** scaffold | 0 | C |
| sera-secrets | **NEW** scaffold | 0 | C |
| sera-workflow | Rewritten M3 | 40 (14 new) | E |
| sera-auth | Extended M3 | 40 (12 new) | E |
| sera-events | Legacy (delete after P0-5) | — | — |
| sera-docker | Legacy (delete after P0-5/P0-6) | — | — |
| sera-db | Unchanged | — | — |
| sera-hooks | Unchanged | — | — |
| sera-hitl | Unchanged | — | — |
| sera-core | Pending rename to sera-gateway (P0-5) | 205 | D |
| sera-runtime | Pending rewrite (P0-6) | 19 | D |
| sera-testing | Unchanged (extend in F) | — | F |
| sera-tui | Unchanged | — | — |
| sera-byoh-agent | Unchanged | — | — |

---

## 5. Design decisions made this session

- **jsonschema 0.46** used instead of 0.38 (plan typo) or 0.28 (doesn't exist). API uses `validator_for(&schema).validate(payload)`.
- **schemars 1.0** used instead of 0.8 (B2 agent found 1.0 available and compatible).
- **casbin 2.x** wired with `DefaultModel::from_str` + `StringAdapter` for policy loading. Real RBAC enforcement works in tests.
- **argon2 0.5** with `password-hash` feature. PHC string format stored in `key_hash_argon2`. No plaintext fallback.
- **sera-queue uses serde_json::Value** on trait methods instead of associated types, keeping QueueBackend object-safe.
- **SandboxPolicy uses untagged serde** for NetworkEndpoint variants (tagged internal serde doesn't support newtype string variants).

---

## 6. Gotchas carried forward

Previous gotchas §6.1–§6.8 from M0 handoff still apply. New additions:

- **§6.9 jsonschema API version sensitivity.** v0.46 uses `jsonschema::validator_for()`, not `JSONSchema::compile()`. If upgrading, check the compile API.
- **§6.10 casbin async model loading.** `DefaultModel::from_str()` is sync but `Enforcer::new()` is async. Don't mix sync/async model construction.
- **§6.11 argon2 password-hash feature.** `argon2 = "0.5"` needs the `password-hash` feature enabled (it is by default). Without it, `PasswordHash::new()` won't exist.

---

## 7. Files that exist and matter

Same as M0 handoff §7, plus:
- **`rust/crates/sera-telemetry/`** — new crate (18 tests)
- **`rust/crates/sera-queue/`** — new crate (12 tests)
- **`rust/crates/sera-tools/`** — new crate (15 tests)
- **`rust/crates/sera-errors/`**, **`sera-cache/`**, **`sera-secrets/`** — scaffolds

---

## 8. Cross-reference map

Carried forward from M0 handoff §8 — unchanged.

---

## 9. Session tooling

- **Task tracking:** Use `bd` (beads) for all task tracking. Run `bd prime` for full workflow context. Do NOT use TodoWrite, TaskCreate, or markdown TODO lists.
- **Knowledge management:** Use `omc wiki` for persistent knowledge across sessions. Significant discoveries, design decisions, and environment quirks should be captured via `wiki add` or `wiki ingest`. Query existing knowledge with `wiki query` before re-investigating known issues.

---

**End of handoff.** A fresh session reading this file can immediately begin Lane D (P0-5 + P0-6 gateway/runtime spine). Lane F follows after D completes.

# SERA 2.0 Audit Round — Session Handoff

> **Purpose:** Bootstrap the next session quickly after the per-crate audit round that landed `IMPL-AUDIT.md`.
> **Date:** 2026-04-12
> **Read this first.** It is the one document a new session needs to rebuild context.
> **Previous handoff** (the spec round): see git history — `git show HEAD~1:docs/plan/HANDOFF.md` if you need the spec-round narrative. The decisions captured there are still authoritative; this file replaces only the "what's next" part.

---

## 1. What this session accomplished

**Single P0 task from the previous handoff: completed.** The "audit `rust/crates/*` against the patched specs" task is done. Output: [`docs/plan/IMPL-AUDIT.md`](IMPL-AUDIT.md) — 869 lines, 14 crates classified, prioritized action list, design-forward obligations checklist.

The audit was produced via parallel orchestration: 1 reference-extraction agent (read SPEC-crate-decomposition) + 4 crate-auditor agents (grouped by domain affinity) running sonnet executors concurrently, then synthesized into the final document. No new specs were read or written this session — only the existing crates were inspected against the already-authored specs.

---

## 2. The audit document — how to use it

[`docs/plan/IMPL-AUDIT.md`](IMPL-AUDIT.md) is structured as:

1. **§1 Target workspace reference** — every crate the spec wants, by layer, with deletions/renames/additions called out. Use this as the "what should exist" reference.
2. **§2 Per-crate audit** — 14 sections, one per existing crate, with classification (`aligned` / `needs-extension` / `needs-rewrite` / `delete` / `missing`), current shape, target shape, and concrete deltas with spec section references.
3. **§3 Missing crates** — 18 crates the target layout requires that don't exist yet, including the new `sera-meta` for self-evolution.
4. **§4 Cross-cutting issues** — sequencing rules, rename ordering, queue extraction, workspace-level dependency additions, design-forward obligations checklist (23 items), code-quality defects.
5. **§5 Prioritized action list** — P0 → P3 mapped to next steps.
6. **§6 Summary table** — single-page overview.

**Read order for the next session:** §1 (target reference) → §6 (summary table) → §4.6 (design-forward obligations checklist) → §5 (action list). Then drill into specific §2 entries only as needed for the crate you're touching.

---

## 3. Key findings (so you don't have to re-read the audit)

### 3.1 Six P0 crate rewrites/extensions

These are Phase 0 blockers, in dependency order:

1. **`sera-domain` → `sera-types`** — first mover. Add all self-evolution primitives (`ChangeArtifactId`, `BlastRadius` (22 variants), `CapabilityToken`, `ConstitutionalRule`, `EvolutionTier`, `AgentCapability`, `BuildIdentity`). Replace `TurnResult` with `TurnOutcome` enum. Replace flat `ChatMessage` with `ConversationMessage { content: Vec<ContentBlock>, cause_by }`. Add `SessionState::Shadow` + 4 other variants. Add 3 `ResourceKind` variants. Patch `ResourceMetadata` and `PersonaSpec`.
2. **`sera-events` → `sera-telemetry`** — wholesale rewrite. OCSF v1.7.0 `AuditEntry` with cryptographic chain, `AuditBackend` trait with `OnceCell<&'static>` static binding (closes trust-collapse attack class), pinned OTel triad, hierarchical `Emitter` namespace tree, `LaneFailureClass` (15 variants).
3. **`sera-config` extensions** — `figment` 0.10, `schemars` 0.8, `jsonschema` 0.38, `SchemaRegistry`, `ShadowConfigStore` overlay, `ConfigVersionLog` with prev/this hash chain, env-var override pattern, layer-merge in `ManifestSet`.
4. **`sera-db` / `sera-queue` split** — extract `LaneQueue` (~597 LoC) to a new `sera-queue` crate, wire `apalis 0.7`, add `MigrationKind` enum (Reversible / ForwardOnlyWithPairedOut / Irreversible), set up `migrations/` directory with sqlx migrate.
5. **`sera-core` → `sera-gateway`** — the largest rewrite. SQ/EQ envelope (`Submission` / `Event` / `Op`), `AppServerTransport` enum (`InProcess | Stdio | WebSocket | Grpc | WebhookBack | Off`) — the architectural spine. `AgentHarness` trait. Two-layer session persistence (sqlx + shadow git). Kill-switch admin socket. `GenerationMarker`. Drop the REST surface as the primary dispatch path (it can wrap as `Submission` emitters).
6. **`sera-runtime`** — `TurnOutcome` adoption (breaking change to trait). `ContextEngine` as a separately pluggable axis (`ContextPipeline` becomes a trait impl). `ContentBlock` propagation. Four-method lifecycle (`_observe` / `_think` / `_act` / `_react`). Compaction pipeline with 9 `Condenser` impls. `Handoff<TContext>` as first-class tool call. Re-plumb the binary `main.rs` as `AppServerTransport::Stdio` (not the old MVS stdin/stdout pattern).

### 3.2 P0 crate that's also a rename + absorption

7. **`sera-docker` → absorb into `sera-tools`** — `sera-tools` does not currently exist as a crate. Stand it up with `SandboxProvider` trait + three-layer policy types (coarse `SandboxPolicy` + `FileSystemSandboxPolicy` + `NetworkSandboxPolicy`), then migrate `ContainerManager` and `DockerEventListener` from `sera-docker` into `sera-tools/src/sandbox/docker.rs` as a `DockerSandboxProvider` impl. Add `regorus`, `SsrfValidator`, TOFU SHA-256 binary identity. **Do not create a peer `sera-sandbox` crate** — the spec puts everything inside `sera-tools`.

### 3.3 P0 workflow + auth

8. **`sera-workflow` rewrite** — current crate is a pre-research skeleton with no execution substrate. Add the full beads-modeled `WorkflowTask` type (including `meta_scope: Option<BlastRadius>` Phase 1 obligation), `WorkflowTaskStatus` (with `Hooked` for atomic claim), `DependencyType` (with `ConditionalBlocks`), content-hash `WorkflowTaskId`, `bd ready` algorithm, atomic `claim_task` protocol with `ClaimToken`/`ClaimError`, termination triad (n_round + is_idle + cost).
9. **`sera-auth` extensions** — design-forward types: `AgentCapability` enum (`MetaChange`/`CodeChange`/`MetaApprover`), `CapabilityToken` with narrowing rule, `Action::ProposeChange(BlastRadius)` + `Action::ApproveChange(ChangeArtifactId)`, `Resource::ChangeArtifact(ChangeArtifactId)`. Real `casbin` 2.19 RBAC. argon2 for `BasicAuthValidator` (replace plaintext API-key comparison). `[features]` block (`default = ["jwt", "basic-auth"]`, `enterprise = ["oidc", "scim", "authzen", "ssf"]`).

### 3.4 Crates classified P1 / P2 / P3

- **P1**: `sera-hooks` (constitutional gate, two-tier bus, wasmtime 43, `updated_input`), `sera-hitl` (7 new subsystems from the 293→531 expansion: `SecurityAnalyzer`, Guardian, `AskForApproval`, `GranularApprovalConfig`, `CorrectedError`, `RevisionRequested`, `MetaChangeContext` with approver pinning, guardrails), `sera-testing` (currently a 4-line stub).
- **P2**: `sera-byoh-agent` (defer until `sera-sdk` lands), `sera-tui` keybinding fix (CLAUDE.md rule violation, independent of phase).
- **P3**: `sera-tui` functional gaps (blocked on `sera-sdk`).

### 3.5 Missing crates (18)

Foundation: `sera-errors`. Infrastructure: `sera-queue`, `sera-cache`, `sera-telemetry`, `sera-secrets`. Core domain: `sera-tools`, `sera-session`, `sera-memory`, `sera-models`, `sera-skills`, **`sera-meta`** (NEW, self-evolution). Interop: `sera-mcp`, `sera-a2a`, `sera-agui`. Clients/SDKs: `sera-sdk`, `sera-cli`, `sera-plugin-sdk`, `sera-hook-sdk`.

### 3.6 Workspace-wide dependency additions

See `IMPL-AUDIT.md` §4.5 for the full block. The load-bearing pins:

```toml
opentelemetry = "=0.27"          # EXACT — load-bearing triad
opentelemetry-otlp = "=0.27"     # EXACT
tracing-opentelemetry = "=0.28"  # EXACT
wasmtime = ">=43, <50"           # loose range — extism MUST NOT be used
```

---

## 4. The next session's first task

Three viable starting points, in priority order. Pick **one**.

### Option A — Phase 0 implementation plan (recommended)

Produce `docs/plan/PHASE-0-PLAN.md` — a code-level breakdown of the P0 work in `IMPL-AUDIT.md` §5. For each P0 item:

- Concrete file/module list (which `.rs` files to create or modify)
- Cargo features that gate what
- Acceptance tests per crate
- Milestone schedule with parallelizable lanes called out
- Sequencing graph (which crates unblock which) — note that `sera-domain` → `sera-types` rename and primitive additions are the first-mover that unblocks everything else

This is the next P0 item from the previous handoff (item 5 in §5). It's bounded, has clear acceptance criteria, and turns the audit into something actionable.

**Recommended opening:**

```
Read docs/plan/HANDOFF.md and docs/plan/IMPL-AUDIT.md §1, §4, §5, §6.
Then produce docs/plan/PHASE-0-PLAN.md per the structure above.
Sequence the work so sera-domain (rename to sera-types) lands first.
For each crate, list the concrete .rs files to create or modify and the
acceptance tests that prove the obligations from §4.6 are present.
```

### Option B — Start the `sera-domain` → `sera-types` first-mover work directly

Skip the plan document and just do the first P0 crate. The audit (§2.1) gives you the full delta list. This works if you trust the audit and want to see real code land before writing more planning docs.

The first-mover changes are:
1. Add `crates/sera-types/` to the workspace (or rename `sera-domain` in place — pick one and document the choice)
2. Create `src/evolution.rs` with `ChangeArtifactId`, `BlastRadius`, `CapabilityToken`, `ConstitutionalRule`, `EvolutionTier`, `ChangeProposer`
3. Create `src/versioning.rs` with `BuildIdentity`
4. Extend `src/capability.rs` with `AgentCapability` enum
5. Patch `src/runtime.rs`: `TurnResult` → `TurnOutcome` enum (this is the breaking change that will cascade to `sera-runtime`)
6. Patch `src/session.rs`: add `SessionState::Spawning`, `::TrustRequired`, `::ReadyForPrompt`, `::Paused`, `::Shadow`
7. Add `src/content_block.rs` with `ContentBlock` enum and `ConversationMessage`
8. Patch `src/config.rs`: add `SandboxPolicy`, `Circle`, `ChangeArtifact` to `ResourceKind`; add `change_artifact` and `shadow` to `ResourceMetadata`; add `mutable_persona` and `mutable_token_budget` to `PersonaSpec`
9. Add `#[non_exhaustive]` to `ResourceKind`, `SessionState`, `BlastRadius`, `EvolutionTier` per SPEC-versioning §5.2
10. Run `cargo check` against the workspace; expect cascading breakage in `sera-runtime` and `sera-core` — that's the signal that the next P0 wave needs to begin

**Caution:** doing the first-mover work without the plan document means the next session has to re-derive ordering for the second wave. Option A is safer if multiple sessions are likely.

### Option C — Knock out the small P1 items in parallel

Three small documentation items from the previous handoff are still pending and can be done by anyone:

- `docs/adr/ACP-A2A-migration.md` (HANDOFF previous §5 item 2) — small, ~1 page, follows BeeAI's playbook
- `docs/plan/VENDORED-PROTOS.md` (item 3) — pin OpenShell + A2A proto commits
- `docs/plan/specs/README.md` index refresh (item 9) — add new specs, drop ACP row

These are not on the critical path, but they're low-effort and clear long-running pending items off the queue. Don't substitute them for Option A or B — do them as side tasks.

---

## 5. Things that did NOT change this session

- All 19 specs in `docs/plan/specs/` are unchanged. The decisions captured in the previous HANDOFF §4 (Gateway↔Harness transport spine, ACP dropped, three-tier self-evolution, handoff-as-tool-call, no workflow DSL, beads as Phase 1 input, separate audit write path, extism rejected, OCSF v1.7.0, AGENTS.md standard) all still hold. They are not re-litigated in this handoff.
- `docs/plan/plan.md` and `docs/plan/architecture.md` are still authoritative and unchanged.
- The 14 existing crates have **not been modified** — only audited. Nothing in `rust/crates/*` changed this session.
- The pending-work items from the previous handoff §5 are still pending **except** P0 item 1 (audit) which is now done. P1 items 2 (ACP→A2A ADR), 3 (proto pinning), 4 (13 JSON schemas), 5 (PHASE-0-PLAN), and the P2/P3 items all remain.

---

## 6. Gotchas and things to know (carried forward)

These are still relevant. From the previous HANDOFF §6:

### 6.1 Hook false-alarms on Edit/Write operations

Spurious `PostToolUse hook: <Tool> operation failed` messages fire even when the tool succeeds. The tool's own confirmation is authoritative. This bit me again this session on the `Write` of `IMPL-AUDIT.md` — the hook said "Write operation failed" but `ls -la` confirmed the 63 KB file was created cleanly. Ignore the hook noise; verify with `ls` or `Read` if uncertain.

### 6.2 OTel triad version lock is load-bearing

`opentelemetry = "=0.27"`, `opentelemetry-otlp = "=0.27"`, `tracing-opentelemetry = "=0.28"` MUST be pinned with exact-equals. Drift produces compile-time trait bound errors. When you create the `sera-telemetry` crate, pin them in the workspace `Cargo.toml` with a doc comment pointing to SPEC-dependencies §8.4.

### 6.3 `wasmtime` ships monthly major bumps

Pin with a loose range `">=43, <50"`, not exact. Revisit quarterly. Component Model and `wasmtime-wasi-http` APIs are stable; `Store` / `ResourceLimiter` occasionally adjusts.

### 6.4 Beads is Go, not Rust

SERA integrates beads by **shelling out to the `bd` CLI** in Phase 1, not as a Rust library dep. The beads data model (`Issue` schema, `bd ready` algorithm) is what's being mirrored in `sera-workflow`'s `WorkflowTask` — but the implementation references the CLI for orchestration.

### 6.5 Context window pressure

The previous session grew to 73%+. This session was much lighter (most work was delegated to parallel sub-agents). When orchestrating audits or wide-fan-out work, prefer sub-agents — they preserve the main context.

### 6.6 New: `sera-domain` vs `sera-types` naming

`CLAUDE.md` records "sera-types → sera-domain" as the MVS alias. The audit recommends doing the rename as the first step of Phase 0 work because every other delta references the spec name `sera-types`. Pick a strategy upfront:
- **Rename in place** — `git mv crates/sera-domain crates/sera-types`, update workspace `Cargo.toml` and every dependent `Cargo.toml`. Atomic, breaks-once, but the diff is large and noisy.
- **New crate alongside** — create `crates/sera-types/` with the new types, gradually migrate `sera-domain` consumers, eventually delete `sera-domain`. Smaller PRs, but `sera-domain` lingers as a transitional layer.

The audit recommends rename-in-place as cleaner, but the PHASE-0-PLAN doc should make this explicit before code lands.

### 6.7 Hook stop-loop on cancel

The cancel skill's stop hook can fire repeatedly after work completes. If you see `[ULTRAWORK #N/50] Mode active` after you think you're done, run `/oh-my-claudecode:cancel` to clear `ultrawork` + `skill-active` state. The cancel skill is the standard exit path.

---

## 7. Cross-reference map (carried forward)

Unchanged from the previous handoff. When a future change hits one of these concerns, look at every listed spec to maintain consistency:

| Concern | Specs that matter |
|---|---|
| New external crate added | SPEC-dependencies (§5–§9), SPEC-crate-decomposition (§3), SPEC-versioning (§5) |
| Gateway↔Harness transport change | SPEC-gateway (§3, §7a), SPEC-runtime (§2.2), SPEC-dependencies (§10.2) |
| Hook point added/removed | SPEC-hooks (§3), SPEC-runtime (§10), SPEC-gateway (§3.2), SPEC-self-evolution (§5.3) |
| Approval scope added | SPEC-hitl-approval (§2, §3, §5a), SPEC-self-evolution (§9), SPEC-identity-authz (§5.1a) |
| Sandbox policy change | SPEC-tools (§6a), SPEC-security (§4), SPEC-dependencies (§10.8, §10.18), SPEC-secrets (§5a) |
| Multi-agent coordination change | SPEC-circles (§3, §5), SPEC-workflow-engine (§4), SPEC-runtime (§9a), SPEC-dependencies (§10.12–§10.17) |
| Memory tier change | SPEC-memory (§2.0), SPEC-runtime (§6a), SPEC-dependencies (§10.16) |
| Self-evolution scope added | SPEC-self-evolution (§9), SPEC-hitl-approval (§5e), SPEC-identity-authz (§5.1b), SPEC-config (§7a, §7b) |
| Audit event added | SPEC-observability (§3.0, §3.1), SPEC-self-evolution (§5.7), SPEC-security (§4.6) |
| **Crate audit / delta question (NEW)** | **`docs/plan/IMPL-AUDIT.md` §2 — per-crate sections** |

---

## 8. Files that exist and matter

- **`docs/plan/HANDOFF.md`** — this file
- **`docs/plan/IMPL-AUDIT.md`** — the new audit document, 869 lines
- **`docs/plan/specs/SPEC-dependencies.md`** — buy-vs-build matrix, 780 lines
- **`docs/plan/specs/SPEC-self-evolution.md`** — three-tier self-evolution, 762 lines
- **17 patched specs** in `docs/plan/specs/`
- **`docs/plan/plan.md`** — PRD, unchanged, still authoritative
- **`docs/plan/architecture.md`** — architecture overview, unchanged, still authoritative

Do not re-read `plan.md` or `architecture.md` cold — they're long and haven't changed. Reference them only when the cross-references in the patched specs point back to specific PRD sections. The same applies to specs: the audit's per-crate deltas already cite the relevant sections — drill in only when you need the surrounding context.

---

**End of handoff.** A fresh session reading this file plus `IMPL-AUDIT.md` §1/§4/§5/§6 should have everything needed to start Phase 0 work.

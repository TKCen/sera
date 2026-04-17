# SERA 2.0 Phase 1+2 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-17
> **Session:** 27 Wave 1+2 close-out (10 beads closed via 10 ultrawork agents)
> **Previous handoffs:** Phase 1+2 session 5 → `git show d02f7f7:docs/plan/HANDOFF.md`; Phase 1+2 session 4 → `git show 6440dca:docs/plan/HANDOFF.md`; Phase 1+2 session 3 → `git show 13f1b6c:docs/plan/HANDOFF.md`; Phase 1 session → `git show 54adaea:docs/plan/HANDOFF.md`; Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. Current state

**Branch:** `sera20` (16 new commits in Session 27 Waves 1-4 on top of Session 26)
**Last commit:** `530f6b5 sera-jwtj: integrate MemoryBlock into sera-runtime context injection path`
**Test count:** ~2,963 (2,867 Session 26 baseline + ~96 Session 27 wave additions; cargo test --workspace exit 0)
**Phase progress:** Phase 2 at 98%, Phase 3 at 85%; sera-commands new foundation crate added; MemoryBlock (2-tier) types landed; WASM hook metering live; Postgres-backed proposal quota; LaneCounterStore trait + Postgres backend (wiring pending).

---

## 1a. Session 27 Wave 1+2 highlights

**10 beads closed across 2 waves (10 parallel ultrawork agents).** Features landed across self-evolution infra, memory types, hooks, skills, and cross-project alignment.

- **sera-pfup** — new `sera-commands` foundation crate (shared CLI ↔ gateway `Command` trait + registry + Ping/Version examples)
- **sera-jj87** — `MemoryBlock` / `MemorySegment` / `SegmentKind` in sera-types (2-tier injection, Soul priority 0 never-evicted, `flush_min_turns=6` pressure signal). Integration into sera-runtime is sera-jwtj follow-up.
- **sera-9p9e** — Hermes-aligned hook-point aliases (`context_memory` → `pre_agent_turn`) via `#[serde(alias)]`, extensible table in `sera-types/src/hook_aliases.rs`
- **sera-kp6e** — skill self-patching scaffold in sera-skills (`SelfPatchValidator` + `Applier` traits, in-memory + FS impls, atomic temp-dir write; version/size/YAML checks)
- **sera-5cj** — Centrifugo thought-stream fix (rename JSON key `event`→`type`, add `ThoughtEvent` struct + per-agent namespaced channel `agent:{id}:thoughts`)
- **sera-jjms** — WASM fuel + memory + wall-clock metering in sera-hooks (wasmtime 26 API: `Config::consume_fuel`, `StoreLimitsBuilder::memory_size`, `tokio::time::timeout`). New error variants `FuelExhausted` / `MemoryLimitExceeded` / `WallClockTimeout`.
- **sera-d54o** — LaneRunGuard SIGTERM race fix (`blocking_lock` synchronous decrement; `post_close_stale_complete_runs` telemetry counter; 3 race regression + 1 integration test)
- **sera-zbsu** — DB-backed ProposalUsageTracker (Postgres `proposal_usage` table + atomic `INSERT … ON CONFLICT DO UPDATE WHERE used < max_proposals`; restart-safe). `AppState.proposal_usage: Arc<dyn ProposalUsageStore>` now trait-object.
- **sera-e8nq** — Postgres LaneQueue `pending_count` backend (`LaneCounterStore` trait + `InMemoryLaneCounter` + `PostgresLaneCounter`; `lane_pending_counts` table with UPSERT). Gateway wiring is sera-bsq2 follow-up.
- **sera-e7xi** — Discord routing 7-hop trace confirms MVS binary is fully wired; most likely root cause of silent bot is operational (MESSAGE_CONTENT intent not enabled in Developer Portal), not architectural. Full report at `docs/plan/discord-routing-investigation-2026-04-17.md`.

### New crate (29th workspace member)

- **sera-commands** — `Command` trait, `CommandRegistry`, example commands. ~260 LOC, 10 integration tests + 1 doctest.

### Breaking / semantic changes

- `AppState.proposal_usage` changed from `Arc<ProposalUsageTracker>` to `Arc<dyn ProposalUsageStore>`. Call sites updated.
- EvolveTokenSigner verify path now accepts tokens signed with the *previous* key within a grace period (sera-occf if that lands mid-Wave-3). Not breaking at the type level; the signer is backward-compatible with any existing token.

### Incident note (for future ultrawork prompts)

Wave 1 hit a working-tree divergence when one agent used `git stash && git reset --hard` to work around `cargo` build-lock contention. Some agent output was temporarily stashed; most was re-written by subsequent agents; the orchestrator recovered by wiring the orphaned files manually. Future dispatch prompts now carry a hard **"NEVER stash, NEVER reset --hard"** constraint — prefer sleep+retry on the lock.

---

## 2. Session 26 highlights

**~63 beads closed across 21 waves of parallel ultrawork.** Features landed across workflow, hooks, runtime, identity/authz, config, gateway, HITL, tools, errors, and foundational infra.

### Features landed (waves 1-6)

- **SPEC-workflow:** AwaitType::Timer gate + ready-queue integration (sera-gks7)
- **SPEC-hooks:** PermissionOverrides + HookCancellation + updated_input propagation (sera-vjyf)
- **SPEC-runtime:** ToolUseBehavior runtime enforcement at act() gate (sera-5soa)
- **SPEC-identity-authz:** Tier-1.5 RoleBasedAuthzProvider + ActionKind (sera-813v)
- **SPEC-config:** commit_overlay snapshot-write-clear + latent drain bug fix (sera-saxv)
- **sera-gateway:** OIDC session seam + intercom agent-ownership + llm_proxy JWT priority (impersonation fix) (sera-xpr1)
- **sera-hitl:** terminal-state guards + is_expired wiring — **BREAKING:** approve/reject/escalate return Result (sera-xxsx)
- **sera-tools:** BashAst <(cmd)/>(cmd) detection + SsrfError::NotAllowed classifier — **BREAKING:** NotAllowed struct variant (sera-nmr9)

### Features landed (waves 7-21)

- **SPEC-workflow:** All 6 AwaitType gates complete — Human/GhRun/GhPr/Change/Mail with per-gate Lookup traits + ReadyContext bundle
- **sera-gateway:** /api/evolve/* full route set (propose/evaluate/approve/apply/get/operator-key); HMAC-SHA-512 CapabilityToken signing; ConstitutionalRegistry YAML seeding + dry-run in /evaluate; Tier-3 operator-key path; max_proposals enforcement; parse_id 500→400 fix; ActingContext gate; SIGTERM graceful shutdown with LaneQueue drain
- **SPEC-identity-authz:** JWT P1 hardening — nbf + iss + aud + configurable leeway (sera-9g7p); CapabilityToken signature verification via HMAC-SHA-512 (sera-pixi); token-id vs proposer_principal cross-check (sera-5bw9)
- **sera-errors:** Unified across 20+ crates via From<> pattern + method form (cache, queue, skills, auth, db, events, config, hitl, hooks, meta, workflow, tools, secrets, session, runtime, telemetry, models, gateway)
- **sera-runtime:** llm_client +37 tests; default_runtime +16 tests; hook-ordering integration test
- **sera-meta:** +41 tests; sera-hitl +29 tests; sera-tools +198 security-focused tests; sera-db LaneQueue +9 tests + gateway shutdown +3
- **Bug fixes:** shadow_store drain() data loss on partial failure; llm_proxy X-Agent-Id impersonation bypass; JWT nbf never validated; parse_id returned 500 on client input

### Test count
- Session 25 end: 1,818 tests
- Session 26 wave-6 sync: 2,455 tests
- Session 26 final (wave 21): 2,867 tests (verified by annotation grep)

### Breaking changes (still active)
- `sera-hitl`: Approval/Rejection/Escalation routes now return `Result<..., ApprovalError>`. Callers must handle errors.
- `sera-tools`: `SsrfError::NotAllowed` is now a struct variant `NotAllowed { reason: String }` instead of unit. Update pattern matches.

---

## 3. Ready follow-ups

Session 27 Waves 1-4 closed 13 beads (see §1a). Remaining queue:

1. **sera-bsq2** — Wire `PostgresLaneCounter` into LaneQueue admission path
   - sera-e8nq landed the standalone store; gateway wiring is the next step
   - Expected to touch `sera-gateway/src/main.rs` + `sera-db/src/lane_queue.rs`

2. **sera-sbh9** — sera-auth CapabilityTokenIssuer
   - Share CapabilityToken type between sera-gateway and agent-runtime
   - Currently duplicated; unify under sera-auth

3. **sera-gateway TraitToolRegistry migration** (not yet a bead)
   - Migrate 14+ Tool-trait adapters from ToolExecutor to TraitToolRegistry
   - Thread ToolContext through ToolDispatcher::dispatch
   - Unlocks tool-level policy enforcement (authorization, rate limits, audit)

4. **Mail gate Design B** (not yet a bead)
   - Deferred decision: pattern-matching vs thread-id for mail gate correlation
   - Spec exists (SPEC-workflow §4.6); needs design decision before implementation

5. **sera-tj02** — Delete Phase 1 legacy main.rs (low-priority cleanup)
   - `sera-gateway/src/main.rs` has Discord code but no consumer; live path is `bin/sera.rs`
   - Needs impact assessment before removing the named binary target

6. **sera-pmzb** — File logging for gateway + Discord REST errors
   - Gateway lacks file appender; Discord `send_message` failures are currently invisible

7. **Semantic-search Tier-2 memory** (extension of sera-jwtj)
   - MemoryBlock Tier-1 is wired; on-demand Tier-2 semantic search via HybridScorer is the next integration layer

---

## 4. Known gotchas

From prior sessions (§4.1–§4.24 from Session 5 handoff still apply). Session 26 additions:

- **§4.25 Breaking change in sera-hitl.** Approval/Rejection routes now return Result. Code calling `approval_router.approve()` must handle `ApprovalError`.
- **§4.26 Breaking change in sera-tools.** `SsrfError::NotAllowed` changed to struct variant. Update all pattern matches from `NotAllowed` to `NotAllowed { .. }`.
- **§4.27 TraitToolRegistry has zero concrete tools.** `ToolRegistry` (ToolExecutor-based, 13 tools) is still the runtime dispatcher. TraitToolRegistry exists as spec-aligned placeholder. Migration is a follow-up task.
- **§4.28 Tier-1.5 ActionKind is gateway-scoped.** RoleBasedAuthzProvider checks `action: ActionKind` at gateway entry. Does not yet thread into tool-level enforcement (needs TraitToolRegistry migration for that).
- **§4.29 EvolveTokenSigner reads key at startup.** HMAC-SHA-512 signing key loaded once on init. Secret rotation requires restart until hot-reload is implemented.
- **§4.30 ProposalUsageTracker is in-memory.** max_proposals enforcement resets on gateway restart. DB-backed tracker is a follow-up.
- **§4.31 sera-oci is a new crate (28th).** Tracker and crate count updated to 28. sera-oci provides OCI image/layer operations; 70 tests.
- **§4.32 JWT leeway is now configurable.** Default leeway changed from 60s to value from config. Callers relying on hardcoded 60s leeway may see tighter validation.
- **§4.33 Signature: `AppState.proposal_usage` is `Arc<dyn ProposalUsageStore>`** (Session 27 sera-zbsu). Production wires `PostgresProposalUsageStore`; tests wire `InMemoryProposalUsageStore`. Counter survives gateway restart.
- **§4.34 New 429 error.** `DbError::QuotaExceeded { token_id, limit }` maps to HTTP 429 Too Many Requests at the gateway. Fired when `check_and_increment` sees `used >= max_proposals`.
- **§4.35 sera-hooks WASM adapter now enforces fuel + memory + wall-clock caps** (Session 27 sera-jjms). Defaults: 10M fuel units / 64 MB memory / 5 s wall time. Configurable via `WasmConfig`. New error variants `WasmError::FuelExhausted` / `MemoryLimitExceeded` / `WallClockTimeout`.
- **§4.36 sera-commands is the 29th workspace crate** (Session 27 sera-pfup). Contains the shared `Command` trait + registry; no migrations of existing CLI/gateway commands yet — that's a follow-up.
- **§4.37 MemoryBlock types live in `sera-types::memory`** (Session 27 sera-jj87). `SegmentKind::Soul` is priority 0 and `render()` never trims it. `record_turn()` increments `overflow_turns` and returns true when `flush_min_turns` is reached — caller emits `memory_pressure`. Runtime integration is sera-jwtj follow-up.
- **§4.38 Hook-point aliases** (Session 27 sera-9p9e). `context_memory` ↔ `pre_agent_turn` accepted interchangeably via serde. Canonical name still serialises as `context_memory`. Table lives in `sera-types/src/hook_aliases.rs`.
- **§4.39 Centrifugo thought-stream JSON key is now `type`** (Session 27 sera-5cj), was `event`. Subscribers must match on `type == "thought_stream"`. Per-agent channel format: `agent:{instance_id}:thoughts`.
- **§4.40 LaneRunGuard Drop is synchronous** (Session 27 sera-d54o). Uses `blocking_lock` rather than `tokio::spawn` to prevent SIGTERM drain race. `LaneQueue::post_close_stale_complete_runs()` exposes a telemetry counter for any residual post-close decrements.
- **§4.41 `LaneCounterStore` trait + `PostgresLaneCounter`** (Session 27 sera-e8nq). Standalone; not yet wired into the runtime LaneQueue admission path. That's sera-bsq2 follow-up.
- **§4.42 Ultrawork prompt must forbid `git stash`/`git reset --hard`.** Wave 1 incident: one agent stashed + reset to unblock a build-lock, losing peer-agent work until orchestrator recovered it manually. All future dispatch prompts include this as a hard constraint.

---

## 5. Crate inventory (28 workspace members)

One new crate added in Session 26 waves 7-21: `sera-oci` (70 tests). All 28 members updated with SPEC-runtime, SPEC-hooks, SPEC-config enforcements.

Key updates:
- **sera-runtime:** ToolUseBehavior enforcement, TraitToolRegistry placeholder, context-aware errors
- **sera-gateway:** OIDC session, intercom, a2a routing, ActionKind authz
- **sera-hitl:** terminal-state guards, Result return types
- **sera-tools:** SSRF detection, BashAst parsing
- **sera-auth:** RoleBasedAuthzProvider (Tier-1.5)
- **sera-workflow:** Timer gates, ready-queue
- **sera-config:** commit_overlay, snapshot mechanics
- **sera-a2a:** New crate (agent-to-agent routing)
- **sera-agui:** New crate (agent UI via SSE)
- **sera-plugins:** API re-exports for external plugins

---

## 6. Bead workflow

Use **bd (beads)** for all task tracking. Do NOT use TodoWrite or TaskCreate.

```bash
bd ready              # See available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim a bead
bd close <id>         # Mark complete (requires reason: "FIXED", "DUPE", "DROPPED")
bd prime              # Full workflow reference
bd remember <text>    # Persistent knowledge (survives session end)
```

**Critical rule:** Work is NOT complete until `git push` succeeds.

---

## 7. OMC wiki

`.omc/wiki/` contains 57+ pages of persistent knowledge.

```bash
wiki query "<text>"   # Search by keyword or tag
wiki read "<page>"    # Read a specific page
wiki_ingest <title> <content> --tags tag1,tag2  # Add/merge knowledge
```

Use wiki to:
- Document non-obvious environment behavior
- Record SPEC interpretations and design decisions
- Capture integration patterns (e.g., how to wire a new gate)

Query before re-investigating known issues.

---

## 8. Design decisions made (Session 26)

- **ToolUseBehavior at act() gate.** Runtime enforces user-set policies (forbidden/allowed/logged) when dispatching tool calls. No ad-hoc bypasses.
- **PermissionOverrides + HookCancellation.** ConstitutionalGate can override permissions and cancel subsequent hooks mid-loop. Replaces previous veto-only model.
- **Tier-1.5 RoleBasedAuthzProvider.** Gateway checks ActionKind per user role at entry. Finer-grained (method-level) authz is a follow-up.
- **OIDC session seam.** Gateway authenticates via OIDC, maps to agent owner. Agent runtime sees authenticated session context (not just JWT).
- **SSRF detection in BashAst.** Bash tool now parses command AST and rejects redirects to non-allowed hosts. Classifier `SsrfError::NotAllowed` for audit.
- **Agent-to-agent routing (a2a).** New sera-a2a crate. Agents can message each other via InProcRouter; capabilities gated per agent pair.
- **Agent UI via SSE.** sera-agui provides real-time agent state updates to web frontend. EventSink abstraction for pluggable backends.
- **HMAC-SHA-512 CapabilityToken signing.** /api/evolve/* tokens are HMAC-signed with a gateway secret. Verified at claim time; token-id vs proposer_principal cross-checked to prevent substitution attacks.
- **ConstitutionalRegistry YAML seeding.** Rules loaded from YAML at startup and merged into registry. Dry-run available via /api/evolve/evaluate before committing proposals.
- **sera-errors From<> adoption pattern.** All crates convert domain errors to SeraError via `impl From<DomainError> for SeraError`. Method form (`.into_sera_error()`) used where orphan rules block From<>.
- **SIGTERM shutdown with LaneQueue drain.** Gateway catches SIGTERM, stops accepting new turns, drains pending LaneQueue items, then exits. Configurable drain timeout.

---

## 9. Files that matter

All from Session 5 handoff §7, plus Session 26 additions:

- **`rust/crates/sera-workflow/src/gates/`** — AwaitType::Timer, ready-queue integration
- **`rust/crates/sera-auth/src/rbac.rs`** — RoleBasedAuthzProvider + ActionKind
- **`rust/crates/sera-config/src/overlay.rs`** — commit_overlay snapshot-write-clear
- **`rust/crates/sera-gateway/src/session.rs`** — OIDC session seam
- **`rust/crates/sera-gateway/src/intercom.rs`** — Agent ownership routing
- **`rust/crates/sera-hitl/src/approval.rs`** — Terminal-state guards, Result return
- **`rust/crates/sera-tools/src/bash.rs`** — BashAst <(cmd)/>(cmd) detection
- **`rust/crates/sera-tools/src/ssrf.rs`** — SsrfError::NotAllowed classifier
- **`rust/crates/sera-a2a/src/`** — Client, InProcRouter, Capabilities
- **`rust/crates/sera-agui/src/sink.rs`** — EventSink, SSE adapter
- **`rust/crates/sera-plugins/src/lib.rs`** — Public API re-exports

---

## 10. Cross-reference map

Carried forward from M0 handoff §10. Check git history for spec interpretations not yet in code.

---

## Next step

A fresh session reading this handoff can:
1. Run `bd ready` to see unstarted work
2. Pick the highest-priority bead (usually lowest ID among ready items)
3. Run `bd update <id> --claim` to own it
4. Consult `.omc/wiki/` for patterns on similar work
5. Implement and test
6. Run `bd close <id>` with reason when done
7. Push to remote at session end (mandatory)

**End of handoff.** Phase 2 at 98%, Phase 3 at 85%. All 6 AwaitType gates live. /api/evolve/* routes live with HMAC-SHA-512 signing. JWT P1 closed. ShadowSessionExecutor is the critical path item for full self-evolution loop.

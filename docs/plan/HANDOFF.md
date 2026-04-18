# SERA 2.0 Phase 1+2 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-17
> **Session:** 28 Wave 1+2 close-out (4 beads closed, 11 new beads filed)
> **Previous handoffs:** Session 27 → `git show 05f3021:docs/plan/HANDOFF.md`; Phase 1+2 session 5 → `git show d02f7f7:docs/plan/HANDOFF.md`; Phase 1+2 session 4 → `git show 6440dca:docs/plan/HANDOFF.md`; Phase 1+2 session 3 → `git show 13f1b6c:docs/plan/HANDOFF.md`; Phase 1 session → `git show 54adaea:docs/plan/HANDOFF.md`; Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. Current state

**Branch:** `sera20` (Session 28 Waves 1+2 on top of Session 27)
**Last commit:** `d5aa3e7 sera-pmzb + sera-tj02: gateway tracing_appender file logging + deleted legacy sera-gateway/src/main.rs`
**Test count:** ~2,963+ (stable from Session 27; +7 LaneQueue↔counter-store seam tests + 3 file_logging tests in Session 28; cargo test --workspace exit 0; sera-gateway 140 tests pass; sera-db 127 tests pass)
**Phase progress:** Phase 2 at 98%, Phase 3 at 85%; CapabilityToken unified under sera-auth; PostgresLaneCounter wired into LaneQueue admission; gateway file logging live; legacy main.rs gone.

---

## 1a. Session 27 highlights

**13 beads closed across 4 waves.** Features landed across self-evolution infra, memory types, hooks, skills, and cross-project alignment. See `git show 05f3021:docs/plan/HANDOFF.md` §1a for the full breakdown.

Notable: sera-commands (29th crate), MemoryBlock 2-tier types, WASM hook metering, Postgres-backed proposal quota, LaneCounterStore trait + PostgresLaneCounter (wiring was pending — completed in Session 28 sera-bsq2).

---

## 1b. Session 28 highlights

**4 beads closed across 2 waves. 11 new beads filed from 3 architect analyses.**

### Wave 1+2 — landed

- **sera-sbh9** — Unified `CapabilityToken` under `sera-auth::capability`. `CapabilityTokenIssuer` service added to sera-auth. Migrated sera-gateway (`evolve_token`, `routes/evolve`, `state`) and sera-meta (`policy`, `constitutional`, `shadow_session`, `artifact_pipeline`) to import from sera-auth. `sera-types::evolution::CapabilityToken` removed.
- **sera-bsq2** — Wired `PostgresLaneCounter` into LaneQueue admission path. Added `LaneCounterStoreDyn` dyn-compatible trait (blanket-impl over `LaneCounterStore`), `LaneQueue::new_with_counter_store()` constructor, and `notify_counter_{increment,decrement}` spawn paths. `bin/sera.rs` auto-selects `PostgresLaneCounter` when DATABASE_URL reachable, `InMemoryLaneCounter` otherwise. 7 new seam tests.
- **sera-pmzb** — Added `tracing_appender` daily-rolling file layer to gateway binary (`bin/sera.rs`). Non-blocking writer; `_guard` held in `main()` so logs flush on exit. Configurable via `SERA_LOG_DIR` (default `./logs`) and `SERA_LOG_LEVEL` (default `info`). Stdout layer preserved. 3 smoke tests in `tests/file_logging_test.rs`.
- **sera-tj02** — Deleted `sera-gateway/src/main.rs` (580 LOC of orphaned Discord scaffolding). Live binary is `src/bin/sera.rs`. Explicit `[[bin]]` target added to `sera-gateway/Cargo.toml`.

### Architect analyses filed (Waves 3-6 queued)

Three architect analyses produced 11 new beads:

- **Mail gate Design B** — Option B (RFC 5322 thread-id primary + SERA-issued nonce fallback); pattern-matching explicitly rejected. One bead: `sera-uwk0`.
- **TraitToolRegistry migration** — Adapter-first strategy (strategy C); authz gated behind `runtime.tool_authz_enabled` feature flag. Five-bead chain: `sera-ilk2` → `sera-26me` → `sera-h7dn` → `sera-sebr` → `sera-cdan`.
- **Tier-2 semantic memory** — pgvector on existing Postgres (not Qdrant, not in-memory HNSW). Four-bead chain: `sera-czpa` → `sera-dmpl` → `sera-0yqq` → `sera-7bc3`.

### P0 bug discovered

- **sera-px3w** — `sera-gateway/src/services/embedding.rs` lines 153 and 161 return `vec![0.0; 384]` on Ollama errors, silently shipping degenerate embeddings. Will be subsumed and fixed by `sera-czpa` (EmbeddingService trait deletes the fallback). Fix should not be applied in isolation.

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
- Session 26 final (wave 21): 2,867 tests

### Breaking changes (still active)
- `sera-hitl`: Approval/Rejection/Escalation routes now return `Result<..., ApprovalError>`. Callers must handle errors.
- `sera-tools`: `SsrfError::NotAllowed` is now a struct variant `NotAllowed { reason: String }` instead of unit. Update pattern matches.

---

## 3. Ready follow-ups

Session 28 Waves 1+2 closed 4 beads (sera-sbh9, sera-bsq2, sera-pmzb, sera-tj02). Queued work:

### P0 Bug

- **sera-px3w** — Silent degenerate embeddings in `sera-gateway/src/services/embedding.rs` (lines 153, 161). Do not fix in isolation — will be subsumed by `sera-czpa` (EmbeddingService trait).

### TraitToolRegistry migration (5-bead chain)

1. **sera-ilk2** — Define `Tool` trait + adapter scaffold
2. **sera-26me** — Migrate first batch of concrete tools
3. **sera-h7dn** — Thread `ToolContext` through `ToolDispatcher::dispatch`
4. **sera-sebr** — Wire authz gate behind `runtime.tool_authz_enabled` feature flag
5. **sera-cdan** — Remove ToolExecutor-based registry; cut over fully to TraitToolRegistry

### Tier-2 semantic memory (4-bead chain)

1. **sera-czpa** — EmbeddingService trait (also fixes sera-px3w degenerate-embedding bug)
2. **sera-dmpl** — pgvector schema + store impl
3. **sera-0yqq** — HybridScorer integration with MemoryBlock Tier-2 path
4. **sera-7bc3** — Tier-2 retrieval wired into sera-runtime context injection

### Mail gate correlator

- **sera-uwk0** — Implement Design B: RFC 5322 `Message-ID` / `In-Reply-To` as primary thread-id; SERA-issued nonce fallback for initial messages. Pattern-matching approach explicitly rejected by architect analysis.

---

## 4. Known gotchas

From prior sessions (§4.1–§4.24 from Session 5 handoff still apply). Session 26 additions:

- **§4.25 Breaking change in sera-hitl.** Approval/Rejection routes now return Result. Code calling `approval_router.approve()` must handle `ApprovalError`.
- **§4.26 Breaking change in sera-tools.** `SsrfError::NotAllowed` changed to struct variant. Update all pattern matches from `NotAllowed` to `NotAllowed { .. }`.
- **§4.27 TraitToolRegistry has zero concrete tools.** `ToolRegistry` (ToolExecutor-based, 13 tools) is still the runtime dispatcher. TraitToolRegistry exists as spec-aligned placeholder. Migration is the sera-ilk2 chain.
- **§4.28 Tier-1.5 ActionKind is gateway-scoped.** RoleBasedAuthzProvider checks `action: ActionKind` at gateway entry. Does not yet thread into tool-level enforcement (needs TraitToolRegistry migration).
- **§4.29 EvolveTokenSigner reads key at startup.** HMAC-SHA-512 signing key loaded once on init. Secret rotation requires restart until hot-reload is implemented.
- **§4.30 ProposalUsageTracker is DB-backed.** `AppState.proposal_usage` is `Arc<dyn ProposalUsageStore>`; production wires `PostgresProposalUsageStore`. Counter survives gateway restart.
- **§4.31 sera-oci is a new crate (28th).** Tracker and crate count updated to 28. sera-oci provides OCI image/layer operations; 70 tests.
- **§4.32 JWT leeway is now configurable.** Default leeway changed from 60s to value from config. Callers relying on hardcoded 60s leeway may see tighter validation.
- **§4.33 Signature: `AppState.proposal_usage` is `Arc<dyn ProposalUsageStore>`** (Session 27 sera-zbsu). Production wires `PostgresProposalUsageStore`; tests wire `InMemoryProposalUsageStore`. Counter survives gateway restart.
- **§4.34 New 429 error.** `DbError::QuotaExceeded { token_id, limit }` maps to HTTP 429 Too Many Requests at the gateway. Fired when `check_and_increment` sees `used >= max_proposals`.
- **§4.35 sera-hooks WASM adapter now enforces fuel + memory + wall-clock caps** (Session 27 sera-jjms). Defaults: 10M fuel units / 64 MB memory / 5 s wall time. Configurable via `WasmConfig`. New error variants `WasmError::FuelExhausted` / `MemoryLimitExceeded` / `WallClockTimeout`.
- **§4.36 sera-commands is the 29th workspace crate** (Session 27 sera-pfup). Contains the shared `Command` trait + registry; no migrations of existing CLI/gateway commands yet.
- **§4.37 MemoryBlock types live in `sera-types::memory`** (Session 27 sera-jj87). `SegmentKind::Soul` is priority 0 and `render()` never trims it. `record_turn()` increments `overflow_turns` and returns true when `flush_min_turns` is reached. Runtime integration landed in Session 27 sera-jwtj.
- **§4.38 Hook-point aliases** (Session 27 sera-9p9e). `context_memory` ↔ `pre_agent_turn` accepted interchangeably via serde. Canonical name still serialises as `context_memory`. Table lives in `sera-types/src/hook_aliases.rs`.
- **§4.39 Centrifugo thought-stream JSON key is now `type`** (Session 27 sera-5cj), was `event`. Subscribers must match on `type == "thought_stream"`. Per-agent channel format: `agent:{instance_id}:thoughts`.
- **§4.40 LaneRunGuard Drop is synchronous** (Session 27 sera-d54o). Uses `blocking_lock` rather than `tokio::spawn` to prevent SIGTERM drain race. `LaneQueue::post_close_stale_complete_runs()` exposes a telemetry counter for any residual post-close decrements.
- **§4.41 `LaneCounterStore` trait + `PostgresLaneCounter` wired** (Session 27 sera-e8nq + Session 28 sera-bsq2). `LaneQueue::new_with_counter_store()` accepts `Arc<dyn LaneCounterStoreDyn>` (blanket-impl over `LaneCounterStore`). `bin/sera.rs` auto-selects backend by DATABASE_URL presence. Multi-pod safe.
- **§4.42 Ultrawork prompt must forbid `git stash`/`git reset --hard`.** Wave 1 Session 27 incident: one agent stashed + reset to unblock a build-lock, losing peer-agent work until orchestrator recovered it manually. All future dispatch prompts include this as a hard constraint.
- **§4.43 `CapabilityToken` canonical home is `sera-auth::capability`** (Session 28 sera-sbh9). `sera-types::evolution::CapabilityToken` is gone. `CapabilityTokenIssuer` service lives in sera-auth. All import sites updated; the old re-export path was removed.
- **§4.44 `LaneQueue::new_with_counter_store()` is the multi-pod constructor** (Session 28 sera-bsq2). `LaneQueue::new()` retains prior in-process-only behaviour. `LaneCounterStoreDyn` is the dyn-compatible wrapper trait; blanket impl covers any `T: LaneCounterStore`. Gateway binary selects `PostgresLaneCounter` when DATABASE_URL is reachable, `InMemoryLaneCounter` otherwise.
- **§4.45 Gateway logs to `./logs/sera.log.{YYYY-MM-DD}` by default** (Session 28 sera-pmzb). Configurable via `SERA_LOG_DIR` + `SERA_LOG_LEVEL`. Non-blocking writer; `_log_guard` held in `main()`. Discord `send_message` error paths now emit structured `tracing::error!` with `channel_id` field. Stdout layer still active.
- **§4.46 `sera-gateway/src/main.rs` is gone** (Session 28 sera-tj02). The sole gateway binary is `src/bin/sera.rs` (target name `sera`), declared via explicit `[[bin]]` in Cargo.toml. Do not reintroduce the old main.rs.

---

## 5. Crate inventory (29 workspace members)

No new crates in Session 28 (still 29). Planned additions from queued work:

- **sera-mail** — mail gate correlator; will land with `sera-uwk0`
- **sera-tier2-*** — potential crates from Tier-2 memory chain (`sera-czpa` onwards); architecture TBD in `sera-czpa`

Session 26 additions for reference: `sera-oci` (28th), Session 27: `sera-commands` (29th).

Key crate notes:
- **sera-auth:** Now owns `CapabilityToken` + `CapabilityTokenIssuer`
- **sera-db:** `LaneQueue` has counter-store integration; `LaneCounterStoreDyn` + `PostgresLaneCounter` live here
- **sera-gateway:** Binary is `src/bin/sera.rs` only; file logging via tracing_appender
- **sera-runtime:** ToolUseBehavior enforcement, TraitToolRegistry placeholder, context-aware errors
- **sera-hitl:** terminal-state guards, Result return types
- **sera-tools:** SSRF detection, BashAst parsing
- **sera-workflow:** All 6 AwaitType gates complete

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

## 8. Design decisions made

### Session 26

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

### Session 28

- **Mail gate thread-id (Design B).** RFC 5322 `Message-ID` / `In-Reply-To` is the primary correlator. SERA-issued nonce fallback for initial messages that lack a thread-id. Pattern-matching on subject/body explicitly rejected.
- **TraitToolRegistry migration: adapter-first (strategy C).** Each existing ToolExecutor tool gets a thin adapter implementing the `Tool` trait. Authz enforcement is gated behind a `runtime.tool_authz_enabled` feature flag so the registry can land incrementally without breaking the live dispatch path.
- **Tier-2 semantic memory: pgvector on existing Postgres.** Qdrant and in-memory HNSW both rejected. pgvector extension on the existing Postgres instance avoids an additional stateful dependency. EmbeddingService trait (sera-czpa) is the entry point and also fixes the silent degenerate-embedding bug (sera-px3w).

---

## 9. Files that matter

All from Session 5 handoff §7, plus Session 26/27/28 additions:

- **`rust/crates/sera-auth/src/capability.rs`** — CapabilityToken + CapabilityTokenIssuer (Session 28 canonical home)
- **`rust/crates/sera-db/src/lane_queue.rs`** — LaneQueue with counter-store integration
- **`rust/crates/sera-db/src/lane_queue_counter.rs`** — LaneCounterStoreDyn, PostgresLaneCounter
- **`rust/crates/sera-gateway/src/bin/sera.rs`** — sole gateway binary entry point; file logging + counter-store wiring
- **`rust/crates/sera-gateway/tests/file_logging_test.rs`** — tracing_appender smoke tests
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

---

## 10. Cross-reference map

Carried forward from M0 handoff §10. Check git history for spec interpretations not yet in code.

---

## Next step

A fresh session reading this handoff can:
1. Run `bd ready` to see unstarted work
2. Pick the highest-priority bead (usually lowest ID among ready items; note `sera-px3w` is P0 but deferred to `sera-czpa`)
3. Run `bd update <id> --claim` to own it
4. Consult `.omc/wiki/` for patterns on similar work
5. Implement and test
6. Run `bd close <id>` with reason when done
7. Push to remote at session end (mandatory)

**End of handoff.** Phase 2 at 98%, Phase 3 at 85%. All 6 AwaitType gates live. /api/evolve/* routes live with HMAC-SHA-512 signing. CapabilityToken unified under sera-auth. LaneQueue multi-pod wiring complete. Gateway file logging live. TraitToolRegistry migration (5 beads) and Tier-2 semantic memory (4 beads) are the critical path items for the next wave.

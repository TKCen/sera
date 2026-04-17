# SERA 2.0 Phase 1+2 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-17
> **Session:** 26 final close-out (21 waves of parallel ultrawork)
> **Previous handoffs:** Phase 1+2 session 5 → `git show d02f7f7:docs/plan/HANDOFF.md`; Phase 1+2 session 4 → `git show 6440dca:docs/plan/HANDOFF.md`; Phase 1+2 session 3 → `git show 13f1b6c:docs/plan/HANDOFF.md`; Phase 1 session → `git show 54adaea:docs/plan/HANDOFF.md`; Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. Current state

**Branch:** `sera20` (~63 commits landed in Session 26 across 21 waves)
**Last commit:** `36e424a sera-jep2: gateway ProcessManager scaffold (SPEC-gateway §18 phase S)`
**Test count:** 2,867 (verified via `#[test]` + `#[tokio::test]` grep across 28 crates; up from 2,455 at wave-6 sync)
**Phase progress:** Phase 2 at 98%, Phase 3 at 85%; all 6 AwaitType gates complete; /api/evolve/* routes live; JWT P1 closed.

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

All Session 26 work was closed. Follow-ups for Session 27 (ordered by priority/dependency):

1. **ShadowSessionExecutor** (sera-yif4 alt)
   - sera-runtime shadow execution path for parallel constitutional validation
   - Prerequisite for full self-evolution loop

2. **DB-backed ProposalUsageTracker**
   - Restart-safe max_proposals enforcement for /api/evolve/propose
   - Currently in-memory; loses state on restart

3. **Secret hot-reload for EvolveTokenSigner**
   - EvolveTokenSigner reads signing key at startup only
   - Needs live rotation support without restart

4. **sera-auth CapabilityTokenIssuer**
   - Share CapabilityToken type between sera-gateway and agent-runtime
   - Currently duplicated; unify under sera-auth

5. **sera-gateway TraitToolRegistry migration**
   - Migrate 14+ Tool-trait adapters from ToolExecutor to TraitToolRegistry
   - Thread ToolContext through ToolDispatcher::dispatch
   - Unlocks tool-level policy enforcement (authorization, rate limits, audit)

6. **LaneRunGuard drop-time race during shutdown exit**
   - Potential race condition surfaced during SIGTERM work
   - Low-priority but should be addressed before production

7. **Postgres LaneQueue pending_count backend**
   - Currently in-memory; needs Postgres backend for multi-instance deployments

8. **Mail gate Design B**
   - Deferred decision: pattern-matching vs thread-id for mail gate correlation
   - Spec exists (SPEC-workflow §4.6); needs design decision before implementation

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

# SERA 2.0 Phase 1+2 — Session Handoff

> **Purpose:** Bootstrap the next session quickly. One file to read to rebuild context.
> **Date:** 2026-04-17
> **Session:** 26 (ultrawork marathon)
> **Previous handoffs:** Phase 1+2 session 5 → `git show d02f7f7:docs/plan/HANDOFF.md`; Phase 1+2 session 4 → `git show 6440dca:docs/plan/HANDOFF.md`; Phase 1+2 session 3 → `git show 13f1b6c:docs/plan/HANDOFF.md`; Phase 1 session → `git show 54adaea:docs/plan/HANDOFF.md`; Phase 0 M2/M4 session → `git show 64031d7:docs/plan/HANDOFF.md`; M0 session → `git show e63a629:docs/plan/HANDOFF.md`; plan round → `git show 216c32c:docs/plan/HANDOFF.md`; M1/M3 session → `git show 7f53126:docs/plan/HANDOFF.md`. Decisions captured there still hold.

---

## 1. Current state

**Branch:** `sera20` (29 commits landed in Session 26)
**Last commit:** `ef6425f sea-ub7z partial: fix workspace clippy --all-targets`
**Test count:** ~2,720+ (up from 1,818; tracker updated in sera-lw2p)
**Phase progress:** Phase 1+2 gates landed; SPEC-workflow, SPEC-hooks, SPEC-runtime, SPEC-identity-authz, SPEC-config all active.

---

## 2. Session 26 highlights

**29 beads closed in a single ultrawork marathon.** Features landed across workflow, hooks, runtime, identity/authz, config, gateway, HITL, tools, and foundational infra.

### Features landed (major)

- **SPEC-workflow:** AwaitType::Timer gate + ready-queue integration (sera-gks7)
- **SPEC-hooks:** PermissionOverrides + HookCancellation + updated_input propagation (sera-vjyf)
- **SPEC-runtime:** ToolUseBehavior runtime enforcement at act() gate (sera-5soa)
- **SPEC-identity-authz:** Tier-1.5 RoleBasedAuthzProvider + ActionKind (sera-813v)
- **SPEC-config:** commit_overlay snapshot-write-clear + latent drain bug fix (sera-saxv)
- **sera-gateway:** OIDC session seam + intercom agent-ownership + llm_proxy JWT priority (impersonation fix) (sera-xpr1)
- **sera-hitl:** terminal-state guards + is_expired wiring + escalate boundary — **BREAKING:** approve/reject/escalate return Result (sera-xxsx)
- **sera-tools:** BashAst <(cmd)/>(cmd) detection + SsrfError::NotAllowed classifier — **BREAKING:** NotAllowed struct variant (sera-nmr9)
- **sera-a2a:** Client + InProcRouter + Capabilities (sera-44pa)
- **sera-agui:** EventSink + SSE adapter (sera-68bf)
- **sera-plugins:** public API re-exports (sera-s34v)

### Test explosion
- Session 25 end: 1,818 tests
- Session 26 end: ~2,720+ tests
- Adds: 900+ tests across SPEC-runtime, SPEC-hooks, SPEC-workflow, identity/authz, gateway, tools, a2a, HITL, plugins.

### Breaking changes
- `sera-hitl`: Approval/Rejection/Escalation routes now return `Result<..., ApprovalError>`. Callers must handle errors.
- `sera-tools`: `SsrfError::NotAllowed` is now a struct variant `NotAllowed { reason: String }` instead of unit. Update pattern matches.

---

## 3. Ready follow-ups

All Session 26 work was closed. Follow-ups for next session (ordered by priority/dependency):

1. **TraitToolRegistry migration** (sera-r8i1 bead text locked)
   - Migrate 14+ Tool-trait adapters from ToolExecutor to TraitToolRegistry
   - Thread ToolContext through ToolDispatcher::dispatch
   - Unlocks tool-level policy enforcement (authorization, rate limits, audit)

2. **sera-gateway SIGTERM graceful shutdown**
   - In progress during Session 26 Lane HH (not closed in ultrawork)
   - Finish signal handling, drain pending turns, coordinated agent shutdown

3. **GhRun/GhPr/Human/Mail/Change gates**
   - All await external integrations (gateway poller, sera-hitl wiring, mail connector, sera-meta change pipeline)
   - Spec exists (SPEC-workflow §4.x), integration sequence TBD

4. **sera-session: WorkflowMemoryManager**
   - Circle coordination for multi-session workflows
   - Not yet built; needed for task handoff across sessions

5. **sera-meta: change_artifact threading**
   - Non-blocking follow-up (sera-vjyf flagged it)
   - Thread change artifacts into gateway pipeline

---

## 4. Known gotchas

From prior sessions (§4.1–§4.24 from Session 5 handoff still apply). Session 26 additions:

- **§4.25 Breaking change in sera-hitl.** Approval/Rejection routes now return Result. Code calling `approval_router.approve()` must handle `ApprovalError`.
- **§4.26 Breaking change in sera-tools.** `SsrfError::NotAllowed` changed to struct variant. Update all pattern matches from `NotAllowed` to `NotAllowed { .. }`.
- **§4.27 TraitToolRegistry has zero concrete tools.** `ToolRegistry` (ToolExecutor-based, 13 tools) is still the runtime dispatcher. TraitToolRegistry exists as spec-aligned placeholder. Migration is a follow-up task (sera-r8i1).
- **§4.28 Tier-1.5 ActionKind is gateway-scoped.** RoleBasedAuthzProvider checks `action: ActionKind` at gateway entry. Does not yet thread into tool-level enforcement (needs TraitToolRegistry migration for that).

---

## 5. Crate inventory (21 workspace members)

No deletions or new crates in Session 26. All 21 members updated with SPEC-runtime, SPEC-hooks, SPEC-config enforcements.

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

**End of handoff.** Phase 1+2 gates live. TraitToolRegistry migration (sera-r8i1) is the critical path blocker for Phase 3.

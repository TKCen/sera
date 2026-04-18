# SERA 2.0 — User-Surface Plan

> **Date drafted:** 2026-04-17 (Session 28 close-out)
> **Working assumption:** backend is ~90% feature-complete but has never been driven end-to-end by a real client. Build the thinnest possible client first, use it as a forcing function to surface integration bugs, then layer TUI and web on top of the same command surface.

---

## 1. Thesis

Three facts drive the ordering:

1. **No client exists.** `legacy/` holds the old TS core, Go CLI, web dashboard, and old TUI. The Rust workspace has a stub `sera-tui` (~2K LOC) and nothing else. Every new feature the backend lands is verified only by crate-local tests.
2. **Cross-crate E2E coverage is thin.** With 2,960+ tests in the Rust workspace, we still cannot demonstrate "user authenticates → spawns agent → agent takes a turn through every gate → evolve proposal lands in HITL → operator approves → memory persists." This is the project's real completion gate.
3. **Phase 3 backend work is valuable but incremental.** Each bead in the TraitToolRegistry and Tier-2 memory chains is 2-3 days. Landing them without a client means accumulating risk: they get tested in isolation, never end-to-end.

The plan front-loads the smallest possible client (a Rust CLI) as the forcing function, paces backend chain work alongside it, and defers the web dashboard until signal from dogfooding makes the UX shape clear.

---

## 2. Gate — local-boot verification

**Before any sprint work, confirm the gateway actually runs locally.** If `cargo run -p sera-gateway --bin sera` does not boot cleanly against a standard docker-compose (postgres + centrifugo + ollama), every downstream plan item is blocked.

**Deliverable:** `sera-boot-verify` (P0). A half-day task. Either confirms a working local stack or files the specific fixes required to get one.

---

## 3. Sprint 1 — CLI MVP (1 week)

**Goal:** `sera chat <agent>` works end-to-end against a local gateway. Dogfood-able.

### CLI beads (priority-ordered)
- **sera-cli-init** (P1) — scaffold new crate `sera-cli` under `rust/crates/`. `clap` subcommand skeleton, config at `~/.sera/config.toml`, uses `sera-commands::Command` trait + registry as the plumbing layer. Single binary target `sera`. One dummy `sera ping` command to prove the wiring.
- **sera-cli-auth** (P1) — `sera auth login` (API-key or OIDC device-flow; pick whichever the gateway's OIDC session already supports) + `sera auth whoami` + `sera auth logout`. Persists token in OS keychain where available, plaintext file as fallback.
- **sera-cli-agent** (P1) — `sera agent list` + `sera agent show <id>` + `sera agent run <id> <prompt>` hitting existing gateway REST endpoints.
- **sera-cli-chat** (P2) — `sera chat <session-id>` interactive REPL. Streams via `sera-agui`'s SSE. Handles tool-call events, HITL-pending events, memory-pressure signals.

### Parallel backend
- **sera-26me** (P1) — TraitToolRegistry bead 2/5: `ToolExecutorAdapter<T>` + register 14 tools in `TraitToolRegistry`. Unblocked by Session 28 ilk2.
- **sera-dmpl** (P1) — Tier-2 bead 2/4: `SemanticMemoryStore` trait + `PgVectorStore` backend. Unblocked by Session 28 czpa.

### Exit criteria
- `sera chat` holds a live session against a local gateway agent
- `sera-26me` closed
- `sera-dmpl` closed
- Test count ≥ Session 28 baseline + 50

---

## 4. Sprint 2 — E2E harness + chain progress (1 week)

**Goal:** one automated integration test that proves the full turn loop works. Backend chains make their middle hop.

- **sera-e2e-harness** (P1) — new `rust/e2e/` directory (or module under `sera-testing`) with at least one integration test that: boots docker-compose → runs `sera auth login` → `sera agent run` → verifies the turn hits at least one hook, produces at least one MemoryBlock segment, and writes an audit row. Gated behind `--features integration`.
- **sera-h7dn** (P1) — TraitToolRegistry bead 3/5: flip `RegistryDispatcher` to `TraitToolRegistry`, delete legacy path.
- **sera-0yqq** (P1) — Tier-2 bead 3/4: `ContextEnricher` promotes Tier-2 hits into MemoryBlock.
- **sera-cli-polish** (P2) — CLI error messages, shell completion, help text, readme.

### Exit criteria
- E2E test passes locally under `cargo test -p sera-testing --features integration`
- Legacy `ToolRegistry` deleted from sera-runtime
- Turn loop demonstrably surfaces Tier-2 semantic recalls

---

## 5. Sprint 3 — TUI maturation + deployment story (1 week)

**Goal:** a terminal-native operator experience, plus a canonical one-command local boot.

- **sera-tui-parity** (P2) — grow `sera-tui` from 2K → enough to show: agent list, session viewer (streaming), inline HITL approve/reject, basic evolve-proposal status. Shares the gateway REST client with `sera-cli` (extract a `sera-client` library if that shape emerges).
- **sera-deploy-compose** (P1) — single authoritative `docker-compose.rust.yaml` at repo root that brings up postgres(+pgvector), centrifugo, ollama, sera-gateway, and optional sera-runtime workers. Replaces the legacy compose files.
- **sera-sebr** (P1) — TraitToolRegistry bead 4/5: wire `AuthorizationProvider` + per-tool authz check (feature-flagged).
- **sera-7bc3** (P2) — Tier-2 bead 4/4: `memory_search` tool + eviction job.

### Exit criteria
- `docker compose -f docker-compose.rust.yaml up` boots the whole stack
- `sera-tui` can approve a HITL request end-to-end
- Per-tool authz can be enabled via config flag

---

## 6. Sprint 4 — stretch / decision point (1 week)

Use the signal from Sprints 1-3 to decide between two paths:

### Path A — web dashboard (if CLI/TUI dogfooding surfaced demand for it)
- Scope: single-page app for agent list + session viewer + evolve-proposal approval. SolidJS or Svelte over the same gateway REST + SSE surface.
- Avoid rebuilding the full legacy dashboard. Ship the 20% that covers 80% of operator needs.

### Path B — backend close-out (if E2E test surfaced hot integration gaps)
- **sera-cdan** (P2) — TraitToolRegistry bead 5/5: convert 10 tools to native `Tool` impls with correct `RiskLevel`.
- **sera-uwk0** (P3) — Mail gate correlator. Only if SMTP infra is actually a near-term need.
- Performance/profiling pass on the turn loop.

---

## 7. What we explicitly do *not* do in this plan

- **No full web dashboard rebuild** — the legacy one is frozen, its scope was too large for the current team cadence.
- **No new workspace crates beyond `sera-cli`** — composition over proliferation. `sera-commands` already exists for command plumbing; reuse it.
- **No Mail gate implementation yet** — depends on SMTP infrastructure that isn't on the near-term roadmap.
- **No re-vivification of `legacy/`** — anything needed from legacy should be ported, not revived.

---

## 8. Risks

1. **Gateway boot failure on fresh checkout** — most likely to surface in the Gate. Mitigation: file specific fixes as blocker beads before Sprint 1.
2. **Gateway API drift vs. CLI expectations** — possible. Mitigation: `sera-cli-agent` written against current OpenAPI spec, not against the legacy Go CLI's assumptions.
3. **pgvector infrastructure friction** — `sera-dmpl` needs a postgres image with the extension. Mitigation: in-memory fallback (already spec'd in the bead) keeps dev unblocked.
4. **Each sprint overruns** — this plan assumes ~1 week each. Realistically a sprint may slip. Preserve the ordering, slip the calendar.

---

## 9. Definition of done for the plan overall

The plan is "done" when a new operator can:

1. Clone the repo
2. Run `docker compose -f docker-compose.rust.yaml up`
3. Run `sera auth login` from the terminal
4. Run `sera chat my-agent` and have a useful conversation
5. See the turn in real-time via `sera-tui`
6. Approve any HITL request inline

At that point, the Rust rewrite has *feature-parity* (for operators) with the archived legacy stack, and the foundation exists for everything else.

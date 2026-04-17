# ADR-0004: Deprecation and Removal of the TypeScript core/ Tree

### Status

Proposed

### Date

2026-04-17

---

### Context

The SERA repository contains two parallel server implementations:

- `core/` — the original TypeScript API server (sera-core), using Bun, Zod, and
  node-pg-migrate. This is the current production implementation.
- `rust/` — the Rust workspace (sera-gateway, sera-runtime, and ~25 library crates)
  introduced as part of the SERA 2.0 migration.

The `core/` directory is not present in the worktree on branch `sera20` (the Rust
migration branch), confirming that the TS implementation lives on the production
branch and that `sera20` is the forward-only Rust track. The goal of this ADR is to
define the conditions under which `core/` is considered superseded and the steps by
which it is retired.

**Why retire core/ rather than maintain both indefinitely?**

Two parallel implementations of the same API surface create compounding costs:

1. Every new feature or bug fix must be applied in two languages and two
   architectural styles. This doubles the implementation effort and introduces
   divergence risk.
2. Integration tests (Playwright E2E in `e2e/`) must continue to pass against both
   stacks during the overlap period, which slows the test suite.
3. Operational documentation, runbooks, and the Docker Compose dev environment must
   describe both paths, increasing onboarding friction.
4. The `sera-types/src/manifest.rs` comment ("The shape must match the Zod schema
   in `core/src/agents/schemas.ts`") indicates that the Rust types are currently
   subordinate to the TS source of truth. Retiring `core/` allows the Rust types
   to become the canonical schema definition.

**Current migration state (inferred from branch `sera20`):**

The Rust workspace has implemented the gateway HTTP layer (`sera-gateway`), the
agent worker (`sera-runtime`), the database access layer (`sera-db`), authentication
(`sera-auth`), sessions, hooks, HITL, workflow, metering, and telemetry. The
services directory has 24 scaffolded modules, several marked with dead-code
allowances pending wiring. The full endpoint surface is approximately 190 paths
(per `docs/openapi.yaml`); the Rust gateway currently implements a subset, with
many endpoints returning stub responses.

A concrete feature-parity checklist is required before retirement can proceed;
this ADR defines the framework for producing and tracking that checklist.

---

### Decision

Retire `core/` in three sequential phases. Each phase has explicit entry criteria
that must be met before the phase begins.

**Phase 1 — Mark (entry: no preconditions; begin immediately)**

Actions:
- Add a top-level `core/DEPRECATED.md` that states the retirement timeline and
  points to the Rust equivalents.
- Add a comment block at the top of `core/src/index.ts`:
  ```typescript
  // DEPRECATED: This implementation is being superseded by rust/crates/sera-gateway.
  // See docs/adr/ADR-0004-legacy-retire.md for the retirement plan.
  // Do not add new features here; file a bead and implement in Rust.
  ```
- Add the same comment header to each major module directory (`core/src/agents/`,
  `core/src/sessions/`, `core/src/auth/`, etc.).
- Update `rust/crates/sera-types/src/manifest.rs` to remove the subordination
  comment and assert instead: "This is the canonical schema definition; the TS
  Zod schema in `core/` must be kept in sync until Phase 3."

Entry criteria for Phase 2: the deprecation notices are merged to main.

**Phase 2 — Freeze (entry: Rust endpoint coverage >= 90% of openapi.yaml paths)**

Actions:
- Enforce a "no new TypeScript" rule: PRs adding new files to `core/src/` are
  blocked by a CI check (`scripts/check-no-new-core-files.sh`) unless they carry
  a `core-exception` label approved by a maintainer.
- Identify the canonical owner of each module:

  | Module area | Canonical owner after Phase 2 |
  |---|---|
  | Agent CRUD, lifecycle | `sera-gateway::routes::agents` + `sera-gateway::services::agent_lifecycle` |
  | Sessions | `sera-session` crate |
  | Authentication | `sera-auth` crate |
  | LiteLLM proxy / model routing | `sera-gateway::services::llm_router` |
  | Hooks | `sera-hooks` crate |
  | HITL | `sera-hitl` crate |
  | Circles, memory | `sera-gateway::services::circle_*`, `sera-gateway::services::memory_manager` |
  | Workflow, scheduling | `sera-workflow` crate |
  | Discord bridge | `tools/discord-bridge/` (out of scope; not a core/ concern) |

- Run `e2e/` Playwright tests against the Rust stack only and verify all pass.
- Pin `core/` package versions; no dependency upgrades permitted.

Entry criteria for Phase 3: all E2E tests pass on the Rust stack, freeze CI check
is green for 30 days, and a named engineer has signed off on the feature-parity
checklist.

**Phase 3 — Delete (entry: Phase 2 criteria met, all production traffic on Rust)**

Actions:
- Remove `core/` directory from the repository in a single commit titled
  `chore: remove retired TypeScript core/ tree`.
- Remove `core/` from `docker-compose.yaml` and `docker-compose.dev.yaml`.
- Remove `node_modules_core` named volume from the dev compose file.
- Remove `web/` workspace references to `core` if any remain.
- Remove the subordination comment from `sera-types/src/manifest.rs`.
- Archive the `core/` tree as a git tag `archive/core-ts-final` before the deletion
  commit, so the history is retrievable.

---

### Alternatives Considered

**A — Keep core/ indefinitely as a reference implementation and fallback**

Rejected. A maintained reference implementation has a real cost: the Zod schemas
and Rust types will diverge, documentation will describe two systems, and new
contributors will be confused about which stack to use. "Reference only" states
tend to persist longer than intended.

**B — Rewrite core/ module by module in the same codebase, keeping both alive
until the last module is migrated (strangler fig pattern)**

The strangler fig pattern is appropriate when the two implementations must serve
live traffic simultaneously with traffic routing (e.g., feature flags per
endpoint). SERA's migration is branch-based: `sera20` is the Rust-only track and
`main` is the TS production branch. The phased deprecation approach in this ADR
is better suited to branch-based migration because it sets clear go/no-go criteria
for the cutover rather than requiring per-endpoint routing infrastructure.

**C — Migrate core/ to Deno or Bun with TypeScript 5 as an intermediate step
before the Rust migration**

Rejected. An intermediate TS modernization step would delay the Rust migration
without delivering the performance, type-safety, or operational benefits that
motivate the migration. Engineering time spent on an intermediate step is
opportunity cost against the Rust implementation.

---

### Consequences

**Positive**

- The Rust types in `sera-types` become the single source of truth for the SERA
  domain model.
- The developer environment becomes simpler: one server binary, one Docker service,
  no Bun workspace for `core/`.
- The CI pipeline no longer needs to test two server implementations.

**Negative / Risk**

- Phase 2 entry criterion (90% Rust endpoint coverage) requires significant
  implementation work in `sera-gateway`. The stub responses in
  `sera-gateway/src/routes/stubs.rs` must be replaced before the freeze can start.
- Any `core/` behavior that is undocumented or only captured in E2E tests may be
  lost if the Rust implementation does not replicate it. A dedicated parity audit
  bead is required (see Followup Beads).
- The `tools/discord-bridge/` sidecar has no Rust equivalent and is explicitly out
  of scope. Its fate must be decided separately.

**Followup work**

- The feature-parity checklist (Phase 2 entry criterion) does not yet exist. It
  must be generated by diffing the openapi.yaml endpoint list against the Rust
  route registrations in `sera-gateway/src/main.rs`.

---

### References

- `rust/crates/sera-types/src/manifest.rs:1–5` — subordination comment to the TS
  Zod schema; to be updated in Phase 1
- `rust/crates/sera-gateway/src/routes/stubs.rs` — stub route handlers representing
  unimplemented endpoints; primary target for Phase 2
- `rust/crates/sera-gateway/src/main.rs` — current Rust route registrations
- `docs/openapi.yaml` — canonical ~190-endpoint surface to be covered before Phase 2
- `docs/RUST-MIGRATION-PLAN.md` — overall migration phases and dependency order
- ADR-0003 — Orchestrator breakup required before agent lifecycle parity is achieved

---

### Followup Beads

- **sera-core-phase1-mark**: Add `core/DEPRECATED.md`, deprecation comment headers
  to `core/src/` module directories, and update the subordination comment in
  `sera-types/src/manifest.rs`.
- **sera-parity-checklist**: Generate the feature-parity checklist by diffing
  `docs/openapi.yaml` against registered Rust routes; file one bead per missing
  route group.
- **sera-core-freeze-ci**: Write `scripts/check-no-new-core-files.sh` and add the
  CI step for Phase 2.
- **sera-core-phase3-delete**: Execute the deletion, docker-compose cleanup, and
  archive tag once Phase 2 criteria are met.

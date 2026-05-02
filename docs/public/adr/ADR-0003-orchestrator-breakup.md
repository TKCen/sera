# ADR-0003: Decomposition of sera-gateway::services::orchestrator

### Status

Proposed

### Date

2026-04-17

---

### Context

`rust/crates/sera-gateway/src/services/orchestrator.rs` is 412 lines and currently
bundles four distinct responsibilities into a single `Orchestrator` struct:

| Responsibility | Representative methods |
|---|---|
| Agent manifest validation | `validate_manifest` |
| Agent lifecycle state transitions | `create_agent`, `start_agent`, `stop_agent` |
| Database persistence | wraps `AgentRepository` directly |
| Container/sandbox management | wraps `SandboxProvider` directly |

The module header comment acknowledges this: "Manages agent creation, startup,
shutdown, and lifecycle transitions. Coordinates between the database, Docker
container manager, and manifest validation."

Several concerns have already begun to compound this breadth:

1. **Pagination is implemented in the service layer** (`list_agents`, lines 281–302)
   rather than the repository, because the `Orchestrator` owns both concerns and
   cross-cutting fixes end up here by default.

2. **Scope validation is absent** (see ADR-0001). The natural place to add it is
   `validate_manifest`, but that function is a private `fn`, not a separately
   injectable component. Adding scope validation here would make the function
   responsible for structural validation, policy lookup, and capability enforcement
   simultaneously.

3. **Unit testing requires a real `PgPool`** because the struct takes the pool
   directly. The existing tests (lines 326–412) exercise only `validate_manifest`,
   the one method that does not touch the database. Tests for `create_agent` and
   `start_agent` cannot be written without a live Postgres connection or the
   infrastructure of `sera-testing`.

4. **The `services/mod.rs` file** lists 24 service modules (as of 2026-04-17),
   several of which (`process_manager`, `session`, `coordination`) will need to
   interact with agent lifecycle events. The current monolithic `Orchestrator`
   creates an implicit coupling point that will grow as those services are wired up.

The services directory already has a `#![allow(dead_code)]` gate noting that many
modules are scaffolded but not yet wired into routes. The decomposition proposed
here is forward-looking: it should be applied before these services are wired into
`AppState` to avoid locking in the monolithic interface.

---

### Decision

Split `Orchestrator` into four focused services, each in its own file under
`sera-gateway/src/services/`:

**1. `manifest_validator.rs` — `ManifestValidator`**

Owns all validation logic for incoming agent manifests. Responsibilities:
- Structural field checks (currently `validate_manifest`).
- Capability parsing: deserialize `spec.capabilities` into `ResolvedCapabilities`.
- Boundary cross-check: load and apply `sandbox_boundary` and `policy_ref` policies
  (as specified in ADR-0001 Phase 1).

Takes no async dependencies; all methods are synchronous and return `Result<_, ManifestValidationError>`. Fully unit-testable without a database.

**2. `agent_registry.rs` — `AgentRegistry`**

Owns the database side of agent management. Responsibilities:
- `create_instance` — persist a validated agent record.
- `get_instance`, `list_instances` — read-path queries.
- `update_status` — write status transitions.
- `exists_by_name` — uniqueness check.
- Pagination at the repository level (removes the in-memory pagination currently
  in `list_agents`).

Takes `PgPool`. No sandbox dependency. Wraps `sera_db::agents::AgentRepository`
with service-level error translation.

**3. `container_manager.rs` — `ContainerManager`**

Owns all sandbox/container interactions. Responsibilities:
- `start` — create and start a container from a resolved config.
- `stop` — destroy a container by handle.
- `build_sandbox_config` — convert an agent database row into `SandboxConfig`
  (env vars, labels, image derivation — currently lines 184–203 of orchestrator.rs).

Takes `Arc<dyn SandboxProvider>` and `DataRoot`. No database dependency. Fully
mockable via `MockSandboxProvider` from `sera-testing`.

**4. `agent_lifecycle.rs` — `AgentLifecycle`**

Thin coordination layer that composes the three services above to implement
named lifecycle workflows. Responsibilities:
- `provision(manifest)` — validate, persist, optionally start.
- `start(agent_id)` — load from registry, call container_manager, update status.
- `stop(agent_id)` — load from registry, call container_manager, update status.
- `terminate(agent_id)` — stop + mark deleted.

Takes `AgentRegistry`, `ContainerManager`, and `ManifestValidator`. This is the
type that `AppState` holds and that route handlers call. It replaces the current
`Orchestrator` type in the public interface.

The existing `orchestrator.rs` file and `OrchestratorError` type remain during the
transition and are deprecated via `#[deprecated]` once `AgentLifecycle` is wired.
The file is deleted in the same PR that removes the last reference.

---

### Alternatives Considered

**A — Keep the monolith, add traits to make it testable (mock injections)**

Considered as a lower-churn path. Rejected because the scope of injected mocks
grows with each new responsibility added to `Orchestrator`. The trait-injection
approach defers but does not eliminate the structural problem. By the time scope
validation (ADR-0001), pagination fixes, and process-manager integration are added,
the struct will be significantly larger and harder to split.

**B — Split into two services: `AgentService` (DB + lifecycle) and
`ContainerService` (sandbox)**

A simpler split than the four-way decomposition proposed here. Rejected because
it keeps manifest validation inside `AgentService`, preventing it from being reused
by a future REST validation endpoint (`POST /api/agents/validate`) without importing
the full DB dependency.

**C — Move lifecycle into `sera-runtime` and keep the gateway as a pure HTTP
adapter**

The `sera-runtime` crate is the agent worker binary. Moving lifecycle management
there would create a bidirectional dependency: the gateway would call into the
runtime, and the runtime would call back to the gateway for status updates. Rejected
as an architectural anti-pattern that the crate split was designed to prevent.

---

### Consequences

**Positive**

- `ManifestValidator` is synchronous and fully unit-testable.
- `ContainerManager` is mockable via `MockSandboxProvider` without a database.
- `AgentRegistry` can grow pagination and indexing logic without pulling in
  sandbox concerns.
- `AgentLifecycle` becomes a thin, readable coordinator — the only type that needs
  integration tests.
- Wiring ADR-0001 scope validation into `ManifestValidator` is additive, not
  invasive.

**Negative / Risk**

- Four files and four error types instead of one. Callers currently importing
  `OrchestratorError` must be updated.
- `agent_lifecycle.rs` must be written carefully to avoid becoming the new
  monolith. The rule: if a method on `AgentLifecycle` exceeds 20 lines, it belongs
  in one of the component services.
- The `services/mod.rs` dead_code allowance must be reviewed once these new modules
  are wired into `AppState`.

**Followup work**

- `AppState` must be updated to hold `AgentLifecycle` instead of `Orchestrator`.
- `orchestrator.rs` must be marked `#[deprecated]` after `AgentLifecycle` is wired.
- Route handlers in `sera-gateway/src/routes/agents.rs` must be updated to call
  `AgentLifecycle` methods.

---

### References

- `rust/crates/sera-gateway/src/services/orchestrator.rs` — the current monolith
  (412 lines)
- `rust/crates/sera-gateway/src/services/mod.rs` — full list of service modules;
  shows the 24 peer services this struct must eventually coordinate with
- `rust/crates/sera-types/src/capability.rs` — `ResolvedCapabilities`, target type
  for manifest capability parsing in `ManifestValidator`
- `rust/crates/sera-db/src/agents.rs` — `AgentRepository`, to be wrapped by
  `AgentRegistry`
- `rust/crates/sera-testing/` — `MockSandboxProvider` for `ContainerManager` tests
- ADR-0001 — scope validation to be wired into `ManifestValidator` Phase 1

---

### Followup Beads

- **sera-manifest-validator**: Extract `ManifestValidator` from orchestrator; add
  capability parsing and boundary checks per ADR-0001 Phase 1.
- **sera-agent-registry**: Extract `AgentRegistry` from orchestrator; move pagination
  down to repository level.
- **sera-container-manager**: Extract `ContainerManager` from orchestrator; write
  unit tests using `MockSandboxProvider`.
- **sera-agent-lifecycle**: Introduce `AgentLifecycle` coordinator; wire into
  `AppState`; deprecate `Orchestrator`.
- **sera-orchestrator-rm**: Remove `orchestrator.rs` once all route handlers are
  migrated to `AgentLifecycle`.

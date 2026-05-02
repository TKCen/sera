# ADR-0001: Capability and Resource Scope Validation

### Status

Proposed

### Date

2026-04-17

---

### Context

SERA agents declare capabilities in their template YAML under `spec.capabilities`
(e.g., `filesystem.write`, `network.outbound`, `exec.commands`). At agent-creation
time the Rust orchestrator (`sera-gateway/src/services/orchestrator.rs`) validates
only three structural fields — `name`, `template_name`, and `image` — and persists
the manifest as raw JSON into the database. No validation of the `capabilities` block
occurs before the record is saved.

At request time, when an agent calls a tool or issues an operator request, no
middleware or service layer checks whether the requested action falls within the
scopes that were declared in the agent's stored manifest. The typed capability
structs exist in `sera-types/src/capability.rs` (`ResolvedCapabilities`,
`FilesystemCapability`, `NetworkCapability`, `ExecCapability`, `ResourceCapability`,
`SecretsCapability`, `AgentCapability`), but nothing reads those fields back from
the database to gate live traffic.

Additionally, delegations stored via `POST /api/delegation/issue` carry a `scope`
field typed as `serde_json::Value`, meaning the scope contract is entirely opaque
to the server — any JSON is accepted without schema-level validation.

This gap allows agents to:

1. Declare any capabilities at creation time without them being cross-checked
   against the referenced `sandbox_boundary` or `policy_ref`.
2. Make tool calls or network requests at runtime that exceed their declared
   capability envelope, because no enforcement layer intercepts them.
3. Receive or issue delegations with arbitrary scope objects that are never
   matched against a known scope vocabulary.

The gap must be closed before the Rust gateway can be considered a safe replacement
for the TypeScript `core/` layer. Security posture and the BYOH contract both assume
that sandbox boundaries are enforced, not just recorded.

---

### Decision

We will add a two-phase scope validation layer:

**Phase 1 — Creation-time structural validation (sera-gateway::services::orchestrator)**

- Parse `spec.capabilities` from the incoming manifest into
  `sera_types::capability::ResolvedCapabilities` using `serde_json::from_value`.
  Return `OrchestratorError::ManifestValidation` on any deserialization failure.
- If `spec.sandbox_boundary` is set, look it up in the `sandbox-boundaries/` YAML
  directory (mounted read-only into the gateway container) and reject any capability
  declared in the manifest that is not listed as `allowed` in that boundary policy.
- If `spec.policy_ref` is set, resolve the corresponding `CapabilityPolicy` from
  `capability-policies/` and apply the same allow-list check.
- Log each validation decision at `tracing::debug!` level with
  `instance_id`, `boundary`, and the list of capabilities checked.

**Phase 2 — Request-time enforcement middleware**

- Introduce `sera-gateway::middleware::scope_guard` (new file).
- On every request that arrives at an agent-scoped route (identified by the
  `X-Agent-Id` or `sera.instance` claim in the auth token), load the agent's stored
  `ResolvedCapabilities` from the database — or a short-lived in-process cache keyed
  by `(instance_id, capabilities_hash)`.
- Match the requested action (tool name, network host, exec command) against the
  loaded capability envelope. Reject with `403 Forbidden` and `SeraError::ScopeDenied`
  if the action is not in scope.

**Delegation scope vocabulary**

- Replace `scope: serde_json::Value` in `DelegationRow` and `IssueDelegationRequest`
  with a typed `DelegationScope` enum (variants: `FullAccess`, `ToolSet(Vec<String>)`,
  `ResourceAccess { read: bool, write: bool }`, `Custom(serde_json::Value)`).
- Add a `DelegationScopeValidator` struct in `sera-gateway::services` that checks
  whether a presented delegation's scope covers the action being requested.

---

### Alternatives Considered

**A — Enforce only at the sandbox boundary (Docker/seccomp), skip application-layer checks**

Rejected. Docker seccomp profiles and network policies enforce OS-level constraints
but cannot express application-level capability semantics (e.g., "this agent may call
the `code_interpreter` tool but not `bash_exec`"). Application-layer enforcement is
required for tool-call gating, delegation scope matching, and audit trails.

**B — Validate capabilities lazily on first use, not at creation time**

Rejected. Lazy validation means a misconfigured agent occupies a database row and
potentially starts a container before the error is surfaced. Fail-fast at creation
time reduces operational noise and prevents agents from reaching the `running` state
with invalid capability declarations.

**C — Keep `scope: serde_json::Value` and enforce via JSON Schema at runtime**

Considered as a lower-effort path. Rejected because JSON Schema validation errors
produce poor diagnostic messages, schema evolution is hard to track in code, and
typed Rust enums give the compiler a chance to catch missing match arms when new
scope variants are added.

---

### Consequences

**Positive**

- Closes the enforcement gap between declared and enforced scopes.
- Provides a structured audit record for every scope check decision.
- Makes delegation scopes machine-readable and testable.

**Negative / Risk**

- Phase 1 is a breaking change for callers that currently pass freeform `capabilities`
  JSON. Existing agent manifests in `agents/` will need to be reviewed for compliance.
- The sandbox-boundary YAML lookup adds a startup-time dependency on the mounted
  config files; tests must use fixtures or the `sera-testing` mock infrastructure.
- Caching resolved capabilities introduces stale-read risk if a capability change is
  applied to a running agent. Cache TTL must be kept short (suggested: 30 s) and
  invalidated on `PATCH /api/agents/{id}`.

**Followup work**

- `sera-gateway::middleware::scope_guard` does not exist yet (see Followup Beads).
- The `AgentRepository` must expose a `get_capabilities(instance_id)` query.
- Integration tests in `sera-testing` need a `MockScopeValidator` to avoid hitting
  the database in unit tests.

---

### References

- `rust/crates/sera-types/src/capability.rs` — typed capability structs
- `rust/crates/sera-gateway/src/services/orchestrator.rs:53–91` — current manifest
  validation (structural checks only)
- `rust/crates/sera-gateway/src/routes/delegation.rs:22,67` — untyped `scope` field
- `sandbox-boundaries/` — tier policy YAML files
- `capability-policies/` — CapabilityPolicy definitions
- `docs/ARCHITECTURE.md` — SERA security model, sandbox tier overview

---

### Followup Beads

- **sera-scope-guard**: Implement `sera-gateway::middleware::scope_guard` (Phase 2
  request-time enforcement).
- **sera-delegation-scope-type**: Replace `serde_json::Value` scope in delegations
  with typed `DelegationScope` enum and add `DelegationScopeValidator`.
- **sera-orchestrator-cap-validate**: Add Phase 1 capability parsing and boundary
  cross-check to `Orchestrator::create_agent`.
- **sera-capabilities-audit**: Add `get_capabilities` repository method and audit
  logging for every scope check decision.

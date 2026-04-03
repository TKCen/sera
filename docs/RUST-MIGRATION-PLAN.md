# SERA Rust Migration Plan

## Purpose

This document proposes a backend-first migration of SERA from the current TypeScript/Bun/Go stack to Rust, with a strong bias toward operational simplicity, type safety, and incremental delivery.

Scope priority:

1. `core/` API server and orchestration logic
2. `core/agent-runtime/` container worker
3. `tui/` terminal UI
4. `web/` only after the backend migration has stabilized

This is intentionally opinionated. SERA is infrastructure software with long-lived processes, concurrency, security boundaries, Docker orchestration, and explicit contracts. Rust is a good fit, but only if the migration preserves delivery velocity and avoids a full rewrite trap.

## Executive Summary

The recommended target architecture is:

- `tokio` for async runtime
- `axum` for HTTP
- `serde` for serialization
- `sqlx` for PostgreSQL access and migrations
- `reqwest` for outbound HTTP
- `tracing` + `tracing-subscriber` + OpenTelemetry for observability
- `bollard` for Docker API access
- `jsonwebtoken` for internal JWTs and `openidconnect` for OIDC flows
- `ratatui` + `crossterm` for the TUI

The key architectural recommendation is to **not** chase a heavy ORM and **not** perform a big-bang rewrite. SERA’s domain is contract-heavy and infrastructure-heavy, not CRUD-heavy. Explicit SQL with compile-time checking is a better fit than Diesel or SeaORM.

The migration should follow a strangler pattern:

- keep the current TypeScript system live
- introduce a parallel Rust `sera-core-rs`
- migrate capability slices, not the entire product at once
- preserve the existing HTTP/OpenAPI and YAML schema contracts
- allow both TypeScript and Rust agent runtimes to coexist during the cutover

## Goals

- Improve safety in concurrency, lifecycle handling, and resource isolation
- Reduce runtime overhead for long-lived orchestration services
- Make domain models more explicit and harder to misuse
- Preserve SERA’s Docker-native, governance-first architecture
- Keep the public API and manifest contracts stable while the implementation changes

## Non-Goals

- Rewriting the web frontend now
- Replacing Centrifugo, PostgreSQL, or Docker Compose during the same migration
- Re-architecting SERA into microservices purely because Rust is being introduced
- Translating TypeScript patterns mechanically into Rust

## Target Rust Architecture

### Repository layout

Add a Cargo workspace at repo root:

```text
rust/
  Cargo.toml
  crates/
    sera-core/
    sera-runtime/
    sera-tui/
    sera-domain/
    sera-db/
    sera-auth/
    sera-events/
    sera-docker/
    sera-config/
    sera-testing/
```

Rationale:

- keep the existing TypeScript and Go code in place during migration
- isolate shared Rust domain types from transport concerns
- prevent `sera-core` from becoming a monolith with internal implicit coupling

### Framework choices

#### Async runtime: `tokio`

Choose `tokio`.

Reason:

- best ecosystem support for HTTP, PostgreSQL, Docker clients, cancellation, timers, channels, and backpressure
- natural fit for SERA’s orchestration and long-lived connection patterns
- strongest hiring and maintenance default in Rust infrastructure work

Reject:

- `async-std`: weaker ecosystem gravity
- custom thread-heavy designs: unnecessary complexity

#### HTTP framework: `axum`

Choose `axum`.

Reason:

- clean composition model on top of `tower`
- strong extractor/middleware story
- straightforward typed state injection
- better fit than Actix for maintainability and ecosystem consistency

Reject:

- `actix-web`: fast, but more specialized and less aligned with the broader `tower` ecosystem
- `warp`: less ergonomic for a large service surface

#### Database access: `sqlx`

Choose `sqlx`, not a classic ORM.

Reason:

- compile-time checked SQL against PostgreSQL
- explicit control over queries and transactions
- better fit for SERA’s current SQL-heavy design, migrations, audit trail, and queue semantics
- lower magic, easier review, clearer performance behavior

Reject:

- `Diesel`: strong type system, but higher friction and worse fit for rapidly evolving infrastructure queries
- `SeaORM`: easier than Diesel, but still adds ORM abstraction where SERA gains little from it

Design rule:

- domain objects live in `sera-domain`
- SQL rows and query code live in `sera-db`
- no leaking `sqlx::Row` or SQL types into handler/business layers

#### Migrations: `sqlx migrate`

Choose `sqlx` migrations for new Rust-owned schema changes.

Important transition rule:

- do not rewrite every historic `node-pg-migrate` migration immediately
- keep the current database schema as the source of truth
- baseline Rust against the existing live schema, then add forward-only SQL migrations from that point

#### Serialization and schemas: `serde`, `serde_yaml`, `schemars`, `jsonschema`

Choose:

- `serde` for JSON/YAML serialization
- `serde_yaml` for manifests
- `schemars` for generating JSON Schema from Rust types where practical
- `jsonschema` for validating external documents against the public schema files

Reason:

- SERA’s manifest formats are public contracts
- Rust structs should not silently diverge from the checked-in schema definitions

Recommendation:

- preserve `schemas/` as canonical public artifacts
- generate comparison tests that ensure Rust models stay aligned with the schema contract

#### HTTP client: `reqwest`

Choose `reqwest`.

Reason:

- mature, reliable, Tokio-native
- good TLS, proxy, streaming, multipart, and timeout support
- appropriate for LLM provider calls, Centrifugo HTTP API, OIDC, and internal service calls

#### Auth and identity

Choose:

- `jsonwebtoken` for internal HS256/RS256 JWT issuance and verification
- `openidconnect` for OIDC login flows and JWKS discovery

Reason:

- internal service identity and external operator auth are different problems
- SERA benefits from a split between low-level internal token work and standards-heavy OIDC flows

#### Docker API: `bollard`

Choose `bollard`.

Reason:

- practical default Docker client for Rust
- supports container lifecycle, networks, logs, stats, and exec operations
- avoids shelling out to the Docker CLI

#### Background jobs

Recommendation: do **not** search for a Rust clone of `pg-boss`.

Choose one of two paths:

1. Short term: keep `pg-boss` in the TypeScript side until the queue boundaries are clearer.
2. Long term: replace it with a Rust-native Postgres queue built on explicit tables plus `FOR UPDATE SKIP LOCKED`.

This plan recommends path 2 for the end state.

Reason:

- `pg-boss` is a good Node solution, but not a good long-term anchor for a Rust system
- SERA’s queue semantics matter enough that owning them explicitly is preferable to depending on a thinly supported compatibility layer
- explicit queue tables improve observability, replay, and operational debugging

Suggested Rust queue model:

- `jobs` table with typed payload versioning
- `available_at`, `attempts`, `max_attempts`, `lease_owner`, `lease_expires_at`
- advisory locking or `SKIP LOCKED` workers
- outbox table for durable event publication when needed

#### Observability: `tracing`

Choose:

- `tracing`
- `tracing-subscriber`
- `tracing-opentelemetry`
- `metrics` or OpenTelemetry metrics depending on deployment needs

Reason:

- SERA has cross-cutting concerns: agent id, task id, acting context, container id, provider name
- structured spans are materially better than plain logs for debugging agent orchestration

#### Error handling

Choose:

- `thiserror` for library and domain error types
- `anyhow` only in binaries, tests, and top-level glue code

Rule:

- no `anyhow` in domain boundaries or HTTP contract code

#### TUI framework: `ratatui`

Choose:

- `ratatui`
- `crossterm`
- `tokio-tungstenite` or `reqwest`/SSE depending on real-time transport needs

Reason:

- Ratatui is the right Rust replacement for Bubble Tea style terminal applications
- stable ecosystem, good rendering model, clear composability

## Component-by-Component Plan

### 1. `core/` -> `sera-core-rs`

This is the largest and most important migration. The Rust version should remain a modular monolith, not a distributed system.

Recommended internal module boundaries:

- `api`: axum routers, extractors, request/response DTOs
- `auth`: API keys, JWTs, OIDC, acting context validation
- `agents`: templates, instances, lifecycle orchestration
- `capability`: policies, boundaries, resolution engine
- `sandbox`: Docker lifecycle and runtime grants
- `llm`: provider registry, routing, budget enforcement hooks
- `metering`: quotas, usage records, aggregation
- `audit`: Merkle-chained event append and verification
- `memory`: knowledge store, embeddings integration, Qdrant client
- `intercom`: Centrifugo publish/subscription support
- `jobs`: scheduling and background workers
- `mcp`: registry and protocol bridges

Important architectural constraint:

- keep one deployable `sera-core-rs` binary until the Rust system has been stable in production for a while
- do not split into services unless a real scaling or failure-isolation issue emerges

### 2. `core/agent-runtime/` -> `sera-runtime-rs`

This is a strong Rust candidate and should move early after the core foundations exist.

Reason:

- the runtime is concurrency-heavy, cancellation-sensitive, and security-sensitive
- Rust materially improves process control, streaming, tool execution, and resource cleanup
- a single statically linked worker binary simplifies container images

Recommended design:

- one binary
- stdin task intake and stdout result protocol preserved initially
- explicit async state machine for reasoning loop
- tool execution isolated behind trait boundaries
- hard timeouts and cancellation propagated through all subprocess operations

Key libraries:

- `tokio::process` for subprocesses
- `reqwest` for LLM proxy calls
- `serde_json` for task/result protocol
- `tracing` for structured task spans

### 3. `tui/` -> `sera-tui-rs`

The TUI is relatively independent and can be migrated after the first stable API surface exists in Rust.

Reason:

- lower infrastructure risk than `core/`
- good candidate for proving shared Rust API client crates
- can consume the same OpenAPI-derived or handwritten client abstractions used in tests and tooling

Recommended design:

- `ratatui` + `crossterm`
- typed API client crate shared with smoke tests
- explicit event loop model with separate tasks for input, network, and render invalidation

### 4. `web/`

Do not migrate the web frontend to Rust now.

Recommendation:

- keep `web/` on TypeScript/React/Vite during the backend migration
- revisit Rust/WASM only if there is a concrete reason, not as a symmetry exercise

Reasons not to move now:

- SERA’s frontend value is product iteration speed, not runtime efficiency
- Rust/WASM will slow the team down on UI work unless the team is already deep in that ecosystem
- React can continue to consume a Rust backend with no architectural penalty

If Rust on the frontend is explored later, evaluate:

- `Leptos` for SSR/full-stack scenarios
- `Yew` only if a component-centric SPA rewrite is truly desired

Neither should block backend work.

## Migration Ordering

### Recommended order

1. Rust foundations and shared contracts
2. `sera-core-rs` read-only capabilities and internal services
3. `sera-runtime-rs` dual-runtime support in the orchestrator
4. Rust ownership of new background jobs and selected write paths
5. Primary API cutover to Rust
6. TUI migration
7. Decommission remaining TypeScript backend pieces

### Why this order

The worst possible order is to rewrite the TUI first, then the worker, then the API, while leaving contracts undefined. That creates three migrations instead of one coordinated program.

The correct order starts with the shared backend contracts:

- database schema behavior
- HTTP request/response behavior
- manifest parsing and validation
- event shapes
- agent runtime protocol

Once those are stable, the runtime can be swapped independently per agent image, and the TUI becomes a straightforward client migration.

## Detailed Phases

### Phase 0: Stabilize contracts before rewriting code

Deliverables:

- freeze the current OpenAPI surface that the web and TUI depend on
- inventory all YAML manifest/schema contracts
- document database ownership by subsystem
- add black-box integration tests around current critical behavior

Required outputs:

- route inventory with stability classification: stable, changing, legacy
- manifest compatibility matrix
- queue/topic inventory: pg-boss queues, Centrifugo channels, audit event types

This phase matters because Rust should replace behavior, not reinterpret it.

### Phase 1: Build shared Rust foundations

Deliverables:

- `sera-domain`
- `sera-db`
- `sera-auth`
- `sera-config`
- `sera-testing`

Implement first:

- config loading
- shared error types
- core domain ids and enums
- manifest types and parsers
- DB pool and transaction helpers
- auth token primitives

Success criteria:

- Rust can parse all current template/agent manifests
- Rust can connect to the current PostgreSQL schema
- Rust can validate internal JWTs and OIDC tokens

### Phase 2: Introduce `sera-core-rs` in shadow mode

Run the Rust core alongside the TypeScript core.

Shadow-mode responsibilities:

- health endpoints
- read-only APIs
- background consumers that do not mutate shared ownership-critical tables yet
- log/metric comparison against the TypeScript system

Examples of safe early slices:

- provider listing
- template listing/get
- audit verification reads
- usage reporting reads
- schema/manifest validation endpoints

Avoid early:

- agent spawn/stop
- queue ownership
- schedule dispatch
- any feature that can create double execution

### Phase 3: Migrate the agent runtime with dual-image support

Modify the orchestrator so an agent instance can specify:

- `runtime: ts`
- `runtime: rust`

This can be implemented through:

- template flag
- image tag resolution
- per-agent override during rollout

Why this should happen before the full API cutover:

- it isolates the runtime rewrite from the control-plane rewrite
- it yields early wins in container startup, memory use, and cancellation safety
- it proves the Rust stack on a bounded component

Required compatibility:

- same LLM proxy contract
- same stdin/stdout task protocol
- same filesystem/tool boundaries
- same Centrifugo publication behavior, or a versioned equivalent

### Phase 4: Move write ownership subsystem by subsystem

Recommended write migration order inside `sera-core`:

1. auth and API key verification
2. provider registry and LLM proxy
3. metering writes
4. audit trail append
5. template and agent CRUD
6. sandbox lifecycle orchestration
7. schedules and job dispatch
8. MCP registry and advanced orchestration paths

Reason:

- start with narrower writes and deterministic side effects
- leave container lifecycle and job dispatch later because they are operationally dangerous

### Phase 5: Switch the public API front door to Rust

At this point, `sera-core-rs` becomes the primary API server.

Transition patterns:

- keep the TypeScript service as a fallback proxy for non-migrated routes
- or put a reverse proxy in front and route path groups to each implementation

Recommendation:

- keep one canonical host and auth surface
- route path groups internally during migration
- do not make clients know about dual backends

### Phase 6: Migrate the TUI

Once the Rust API is primary and stable:

- build `sera-tui-rs`
- reuse the typed Rust client
- match the current UX before extending it

### Phase 7: Remove TypeScript backend services

Decommission only when:

- all write paths are owned by Rust
- dual-run comparisons are clean
- rollback path has been practiced

## Coexistence Strategy During the Transition

This is the most important part of the migration.

### Dual-stack principles

- one shared PostgreSQL database
- one shared Centrifugo deployment
- one shared Docker daemon
- one canonical external API hostname
- explicit subsystem ownership at any moment

### Ownership rules

At any time, each of these must have exactly one writer:

- agent lifecycle tables
- job dispatch tables
- audit append path
- token metering counters
- schedule execution path

Read-sharing is fine. Write-sharing without idempotency and leases is not.

### API coexistence

Recommended approach:

- maintain the current HTTP contract
- use path-based or feature-based routing between Node and Rust
- keep response shapes byte-for-byte compatible where practical

Suggested transitional topology:

```text
Clients
  -> gateway/front door
    -> sera-core-ts for legacy routes
    -> sera-core-rs for migrated routes
```

If a separate gateway is undesirable, keep the existing TypeScript core as the front door temporarily and proxy migrated routes to Rust from there.

### Database coexistence

Rules:

- no ad hoc shared writes
- every table gets an owner label during migration
- schema changes must be backward-compatible until the old writer is removed
- prefer additive migrations, not destructive renames

Recommended techniques:

- additive columns
- versioned payloads
- database views to preserve old shapes if needed
- idempotency keys on external-effecting operations

### Runtime coexistence

Support both worker images in the orchestrator:

- `sera-agent-worker-ts`
- `sera-agent-worker-rs`

Rollout strategy:

- internal test agents first
- one builtin template next
- operator-selected opt-in
- default to Rust only after production confidence

### Event and message compatibility

Version these explicitly:

- audit event payloads
- Centrifugo publication payloads
- job payloads
- runtime task/result payloads

Rule:

- if payload semantics change, add a version field instead of silently reinterpreting old messages

## Type System Mapping: TypeScript to Rust

This migration will fail if the team treats Rust as “stricter TypeScript”. It is not.

### Core mapping principles

- TypeScript unions -> Rust `enum`
- optional properties -> `Option<T>`
- string literal unions -> Rust `enum` with `serde` rename rules
- object maps -> `HashMap<K, V>` or dedicated structs if the shape is actually known
- discriminated unions -> tagged `enum`
- `Date`/ISO strings -> `time::OffsetDateTime` or `chrono::DateTime<Utc>`
- nullable vs optional must be modeled separately

### Recommended Rust modeling rules

#### 1. Replace “bag of options” types with enums

Typical TypeScript problem:

```ts
type Job = {
  status: 'queued' | 'running' | 'failed' | 'done';
  error?: string;
  result?: string;
};
```

Preferred Rust shape:

```rust
enum JobState {
    Queued,
    Running,
    Failed { error: String },
    Done { result: String },
}
```

This eliminates invalid states that TypeScript often permits.

#### 2. Separate external DTOs from internal domain types

Do not use one struct for:

- database row
- HTTP payload
- in-memory domain model

Rust makes these separations cheap and worth it.

#### 3. Make state transitions explicit

Good Rust targets in SERA:

- agent lifecycle state machine
- permission request lifecycle
- schedule dispatch lifecycle
- runtime task execution lifecycle

These should be encoded as enums and validated transitions, not stringly-typed updates.

#### 4. Encode capability resolution as data, not loose maps

The current TypeScript capability system is a strong candidate for Rust enums and structs with exhaustive matching. This is one of the areas where Rust will provide the biggest correctness gain.

### Practical mapping examples

#### Zod schemas -> Serde + schema tests

Current TypeScript:

- `zod` validates API and manifest input

Rust target:

- `serde` for deserialization
- custom validators for semantic rules
- schema conformance tests against checked-in JSON Schema

#### `Promise<Result | null>` -> `Result<Option<T>, E>`

Rule:

- absence and failure must not be collapsed into the same thing

#### `Record<string, unknown>` -> avoid by default

Use:

- `serde_json::Value` only at external edges
- typed structs as soon as data crosses into business logic

## Testing Strategy for the Migration

The migration needs compatibility tests, not just new unit tests.

### Test layers

#### 1. Contract tests

For each migrated route or manifest parser:

- run the same input corpus against TypeScript and Rust
- compare status code, response shape, and important side effects

This is the highest-value test category during migration.

#### 2. Golden tests

Use golden files for:

- manifest parsing/normalization
- system prompt assembly
- capability resolution outputs
- audit event serialization

#### 3. Integration tests

Rust integration tests should use real infrastructure for:

- PostgreSQL
- Centrifugo where applicable
- Qdrant
- Docker, for runtime and sandbox tests

Prefer `testcontainers` where practical, but do not force it if plain Compose is simpler for this repo.

#### 4. Shadow traffic comparison

For safe read paths:

- mirror selected requests to Rust
- compare outputs without serving Rust responses yet

#### 5. Dual-runtime e2e tests

The e2e suite should exercise:

- TS core + TS runtime
- TS core + Rust runtime
- Rust core + Rust runtime

This catches protocol drift early.

### Recommended Rust test stack

- built-in `cargo test`
- `insta` for snapshot/golden tests
- `testcontainers` optionally for isolated integration environments
- `wiremock` for HTTP dependency mocking
- `proptest` for capability resolution, policy merging, and parser invariants

### Specific high-risk areas that need property or exhaustive tests

- capability resolution
- manifest merging and override precedence
- audit chain integrity
- budget enforcement math
- queue lease/ack/retry behavior
- runtime cancellation and timeout cleanup

## Risk Assessment

### High risk

#### 1. Big-bang rewrite risk

Risk:

- the team disappears into infrastructure work for months
- parity slips
- user-facing development stops

Mitigation:

- strangler migration
- subsystem ownership
- route-by-route and runtime-by-runtime cutovers

#### 2. Schema and contract drift

Risk:

- Rust models diverge from existing API or YAML behavior
- frontend and templates break subtly

Mitigation:

- contract tests
- schema conformance tests
- public contract freeze before migration

#### 3. Queue and scheduling semantics

Risk:

- duplicate jobs
- lost retries
- schedules firing twice

Mitigation:

- single-writer ownership
- leased jobs
- idempotency keys
- delayed migration of scheduling/dispatch until later phases

#### 4. Docker orchestration behavior differences

Risk:

- container networking, log streaming, and cleanup semantics differ from the current implementation

Mitigation:

- e2e tests on real Docker
- migrate orchestration later, after read paths and runtime are stable

#### 5. Team learning curve

Risk:

- Rust velocity initially lower than TypeScript
- codebase becomes “technically better” but slower to evolve

Mitigation:

- small shared crates
- strict review guidelines
- avoid advanced type-level cleverness
- prefer explicit, boring Rust

### Medium risk

#### 1. Library churn in lesser-used areas

Especially:

- OIDC support details
- Docker client edge cases
- WebSocket or Centrifugo client behavior

Mitigation:

- wrap external crates behind local interfaces

#### 2. Build and CI complexity

Risk:

- mixed Bun, Go, and Cargo pipelines during transition

Mitigation:

- treat Rust as an added lane first, not an immediate replacement
- use separate validation stages until cutover

### Low risk

#### 1. TUI migration

This is operationally low-risk once the backend API is stable.

## Performance Expectations

Rust will help, but the gains will vary by component.

### `sera-core`

Expected improvements:

- lower baseline memory usage than Node for the same long-lived service footprint
- lower tail latency under concurrent orchestration workloads
- better cancellation behavior and less accidental work retention
- improved CPU efficiency in JSON processing, policy evaluation, and audit hashing

Realistic expectation:

- 20-40% lower steady-state memory for equivalent features
- 10-30% lower p95 latency on CPU-heavy or serialization-heavy endpoints
- little change on pure network-bound paths dominated by PostgreSQL or upstream LLM latency

### `sera-runtime`

Expected improvements:

- faster cold start with a static binary than a Bun-based runtime plus JS startup
- tighter memory profile inside agent containers
- better subprocess and stream handling

Realistic expectation:

- materially lower idle memory per worker container
- noticeably safer cancellation and shutdown behavior
- total task latency still dominated by model inference in many cases

### `sera-tui`

Expected improvements:

- mainly maintainability and codebase consistency, not raw performance
- startup and binary distribution improve

### What will not improve much

- LLM response latency
- PostgreSQL round-trip latency
- Docker daemon latency
- Centrifugo network latency

The migration is mostly about correctness, resource efficiency, and operational confidence, not magic speedups.

## Recommended First Milestone

The first meaningful migration milestone should be:

- Cargo workspace added
- shared Rust domain/auth/db crates in place
- Rust manifest parser compatible with current YAML
- Rust health/read-only API process running beside the TypeScript core
- dual-runtime support implemented for agent containers
- one internal agent template running on `sera-runtime-rs`

Why this milestone:

- proves the tooling
- proves coexistence
- delivers real operational value
- does not yet bet the product on a full cutover

## Opinionated Decisions Summary

- Use `axum`, not Actix.
- Use `tokio`, not alternative runtimes.
- Use `sqlx`, not Diesel or SeaORM.
- Keep PostgreSQL and Centrifugo.
- Replace `pg-boss` eventually with an explicit Rust-owned Postgres queue.
- Migrate the runtime before the full control plane cutover.
- Keep the web frontend on TypeScript/React for now.
- Preserve public API and manifest contracts while implementation changes underneath.
- Favor a modular monolith, not service sprawl.

## CCG Synthesis: Cross-Advisor Perspectives (2026-04-02)

This section captures where multiple advisor perspectives (Codex, Gemini, Claude) agreed and disagreed, to inform final decisions.

### Universal agreement

All three advisors converged on:

- `axum` + `tokio` + `sqlx` + `reqwest` + `bollard` as the core stack
- Strangler fig pattern for sera-core migration
- Keep `web/` on TypeScript/React — Rust/WASM UI frameworks are not ready
- `ratatui` + `crossterm` for the TUI
- `tracing` + `tracing-subscriber` for structured observability
- Modular monolith architecture — do not split into microservices

### Open question: Should the agent-runtime migrate to Rust?

**Position A (Codex, this plan):** Migrate early — the runtime is small, isolated, and benefits from Rust's memory safety, faster cold start (~100ms vs ~2s), and smaller container images (~10MB vs ~200MB).

**Position B (Gemini):** Keep Bun for agent containers — agents need to execute arbitrary JS/TS skills, and compiling every skill to Rust is impractical. The runtime's job is to be a lightweight host for user-defined logic, not a performance-critical path.

**Recommended resolution:** Migrate the runtime binary itself to Rust, but support a **polyglot tool executor** — Rust handles the reasoning loop, HTTP client, and context management, while user-defined skills/tools can be JS/TS scripts executed via subprocess (`tokio::process`). This gives the cold start and memory wins without requiring skills to be written in Rust.

### Job queue: Custom vs `apalis`

**This plan recommends** a custom Postgres queue with `FOR UPDATE SKIP LOCKED`.

**Gemini recommends** `apalis` — a Tower-based job queue crate with Postgres/SQLx backend, ~90% parity with pg-boss.

**Recommendation:** Evaluate `apalis` during Phase 1. If it covers SERA's queue semantics (retry, scheduling, lease expiry, dead-letter), use it. If not, the custom approach is straightforward and gives full control. Do not spend more than 1 week evaluating.

### Timeline reference estimates (from Gemini)

These are rough dev-week estimates for a developer already comfortable with Rust:

| Component                           | Complexity | Estimated Dev-Weeks |
| ----------------------------------- | ---------- | ------------------- |
| API surface (leaf/read-only routes) | Low        | 1                   |
| Database layer (sqlx port)          | Medium     | 2                   |
| LLM router + provider registry      | Medium     | 2                   |
| Orchestrator + sandbox manager      | High       | 4                   |
| TUI (ratatui rewrite)               | Medium     | 2                   |
| **Total**                           |            | **~11**             |

Add 30-50% for a team still ramping up on Rust. These exclude Phase 0 contract stabilization.

### Additional DX recommendations (from Gemini)

- **Linker:** Use `mold` (Linux) or `lld` (Windows) for significantly faster link times during development
- **Debugging async:** Use `tokio-console` for inspecting task contention, deadlocks, and slow futures
- **Rosetta Stone document:** Maintain a living mapping of `TypeScript module -> Rust crate/module` (e.g., `PgBossService.ts -> sera-jobs/queue.rs`) to help the team navigate during transition
- **LLM client:** Consider `async-openai` crate for the LLM proxy client — mature, well-maintained, OpenAI-compatible API

---

## Recommended Next Steps

1. Create a migration epic with the phases in this document.
2. Freeze and inventory current contracts: OpenAPI, YAML schemas, queue payloads, Centrifugo payloads.
3. Add a Cargo workspace and foundational crates.
4. Build compatibility tests against the current TypeScript implementation.
5. Implement `sera-runtime-rs` behind dual-image support.
6. Introduce `sera-core-rs` in shadow mode for read-only routes.

If the team follows this sequence, SERA can move to Rust without stopping feature work or accepting a rewrite-sized outage in delivery.

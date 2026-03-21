# SERA Testing Strategy

This document defines the cross-cutting test strategy for the SERA codebase. All agents implementing epics should follow these conventions. Consistency matters more than per-story cleverness — tests are read more often than written.

---

## Test categories

### Unit tests
Pure logic with no I/O. No database, no Docker, no HTTP. Fast (< 5ms per test).

**What belongs here:**
- Capability resolution logic (`CapabilityResolver.resolve()`)
- `$ref` expansion and cycle detection in NamedLists
- Token budget arithmetic
- Credential resolver priority ordering
- ActingContext chain validation
- Prompt delimiter wrapping
- Context window token estimation and compaction decisions
- Schedule cron expression parsing and next-run calculation
- Merkle hash-chain integrity verification
- Zod schema validation logic

**Framework:** Vitest (Node.js native, TypeScript, compatible with ESM, fast watch mode)

### Integration tests
Tests that require a real database or real infrastructure services but do not require Docker agent containers. Run against a dedicated test database.

**What belongs here:**
- `CapabilityPolicy` and `NamedList` import-on-load (file → DB)
- Agent template and instance CRUD (PostgreSQL)
- Secret store encrypt/decrypt round-trip (PostgreSQL)
- Task queue enqueue/dequeue/status transitions
- Audit trail Merkle chain: insert → verify
- `KnowledgeGitService`: write → commit → merge → Qdrant re-index
- OIDC token validation against a mock JWKS endpoint
- `CredentialResolver` against a real SecretsProvider

**Infrastructure:** Docker Compose test profile (`docker-compose.test.yaml`) that starts PostgreSQL, Qdrant, and a mock OIDC server (see below). Does **not** start LiteLLM, Centrifugo, or agent containers.

**Database:** Separate `sera_test` database. Migrations run before test suite; truncated (not dropped) between tests for speed.

**Framework:** Vitest with `@vitest/coverage-v8`

### End-to-end tests
Tests that start the full stack including sera-core, spawn a real Docker agent container, and verify observable outcomes.

**What belongs here:**
- Agent spawn → heartbeat → task execution → result → teardown
- LLM proxy: JWT validation, budget enforcement (using a mock LiteLLM)
- Permission request flow: agent requests → operator approves → agent unblocks
- Secret injection: secret created → agent spawned → secret available in tool call
- Webhook delivery: POST to `/webhooks/:id` → task enqueued for agent

**Infrastructure:** Full `docker-compose.yaml` stack. LiteLLM replaced with a mock HTTP server that returns scripted responses. A real Docker socket is required (CI must run with Docker available).

**Framework:** Vitest + custom Docker lifecycle helpers. E2E tests tagged `@e2e` — excluded from standard `bun test`, run explicitly in CI via `bun run test:e2e`.

---

## Infrastructure for tests

### Test database
```bash
# Create test DB (run once)
createdb sera_test
# Migrations applied automatically by test setup hook
```

`vitest.config.ts` global setup runs migrations against `sera_test` before the suite.

### Mock OIDC server
Use `@axa-fr/oidc-client-mock` or a lightweight custom JWKS endpoint (`jose` can generate test key pairs) — do not depend on a live Authentik instance for tests.

Test key pair generated once per suite; `OIDC_ISSUER_URL` points to the mock server.

### Mock LiteLLM
A simple HTTP server (Fastify) that implements `POST /v1/chat/completions` and `GET /model/info`. Responses are scripted per test. Used in E2E tests only.

### Docker in CI
E2E tests require `DOCKER_SOCKET_PATH` to be available. CI must run on a host with Docker (not Docker-in-Docker for socket tests). GitHub Actions: use `ubuntu-latest` runners with Docker pre-installed.

---

## Test data conventions

**No shared mutable fixtures.** Each test creates its own data and cleans up (or relies on transaction rollback).

**Naming:**
```
describe('CapabilityResolver', () => {
  describe('resolve()', () => {
    it('boundary alone — no policy, no inline overrides', ...)
    it('inline broadening beyond policy raises CapabilityEscalationError', ...)
  })
})
```

**Agent test fixtures:** A minimal `AgentTemplate` factory function in `test/fixtures/templates.ts`. Tests import and customise it — no copy-pasted YAML in test files.

---

## Coverage targets

| Layer | Target | Rationale |
|---|---|---|
| Capability resolution | 100% branch | Core security invariant — deny-wins must be exhaustively tested |
| Secret encryption/decryption | 100% branch | Data integrity and security |
| JWT issuance and validation | 100% branch | Auth correctness |
| API endpoints | 80% line | Happy path + principal error cases per endpoint |
| Agent runtime reasoning loop | 70% line | LLM responses are scripted in tests; tool executor is unit-tested separately |
| UI components | No target | Covered by E2E; unit testing individual React components has low ROI here |

Coverage enforced in CI via `--coverage --coverage-threshold` in Vitest config. Build fails if thresholds are not met.

---

## What NOT to test

- The LiteLLM service itself — it is a dependency, not code we own
- Centrifugo channel delivery — mock the `IntercomService.publish()` call at the boundary
- Docker API calls in unit tests — mock `dockerode` using `vi.mock('dockerode')`; real Docker is only exercised in E2E
- git operations in unit tests — mock `simple-git`; real git used in integration tests for `KnowledgeGitService`
- UI rendering of every state permutation — test the data flows, not the pixels

---

## Test file locations

```
sera-core/
  src/
    capability/
      resolver.ts
      resolver.test.ts          ← unit tests colocated with source
  test/
    integration/
      capability-import.test.ts
      secret-store.test.ts
      knowledge-git.test.ts
    e2e/
      agent-lifecycle.test.ts
      permission-request.test.ts
    fixtures/
      templates.ts
      secrets.ts
      operators.ts
    setup/
      database.ts               ← migration runner for test DB
      mock-oidc.ts
      mock-litellm.ts
```

Unit tests are colocated with source (`*.test.ts` alongside `*.ts`). Integration and E2E tests live in `test/`.

---

## CI pipeline

```yaml
# Runs on every PR
test:unit:
  run: vitest run --reporter=verbose

test:integration:
  needs: [docker-compose test profile healthy]
  run: vitest run --reporter=verbose --testPathPattern=test/integration

# Runs on merge to main only (slower, requires Docker)
test:e2e:
  needs: [full stack healthy]
  run: vitest run --reporter=verbose --testPathPattern=test/e2e
```

PR checks require unit + integration to pass. E2E runs on merge to main. This keeps PR feedback fast (< 2 min) while ensuring the full stack is validated before release.

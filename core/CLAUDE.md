# sera-core

Central API server, LLM proxy, orchestrator, and governance layer. See `docs/ARCHITECTURE.md` → Component Architecture for the full module design and `docs/epics/` for story-level acceptance criteria.

## Tech stack

| Concern | Library |
|---|---|
| Runtime | Node.js 22 LTS |
| HTTP framework | Express 5 (current) → Fastify v5 (planned — check `docs/ARCHITECTURE.md` → Tech Stack for migration status) |
| Validation | zod |
| JWT | jose v5 |
| Database | postgres.js + PostgreSQL |
| Job queue | pg-boss |
| Docker API | dockerode |
| Git operations | simple-git |
| Embeddings | Ollama (`nomic-embed-text`) |
| Tests | Vitest |

## Binary paths

`npx` does not resolve local binaries in this environment. Always use full paths:

```bash
# TypeScript — -p flag is required; omitting it causes tsc to print help and exit 1
D:/projects/homelab/sera/core/node_modules/.bin/tsc --noEmit -p D:/projects/homelab/sera/core/tsconfig.json

# Vitest — pass file paths as positional args; --root flag is not supported
D:/projects/homelab/sera/core/node_modules/.bin/vitest run core/src/path/to/file.test.ts

# Run all tests
D:/projects/homelab/sera/core/node_modules/.bin/vitest run
```

## TypeScript strict flags (`core/tsconfig.json`)

Two non-standard flags that cause non-obvious errors:

**`exactOptionalPropertyTypes: true`** — cannot assign `undefined` to an optional property:
```ts
// Wrong
const obj = { foo: maybeUndefined }
// Right
const obj = { ...(maybeUndefined !== undefined ? { foo: maybeUndefined } : {}) }
```

**`noUncheckedIndexedAccess: true`** — array and object index access returns `T | undefined`:
```ts
const item = arr[0]!        // use non-null assertion
const val = map['key']!     // or guard with an if-check
```

## Source module map (`src/`)

| Directory | Purpose |
|---|---|
| `agents/` | AgentFactory, instance management, Sera bootstrap |
| `audit/` | Merkle hash-chain audit trail |
| `auth/` | AuthPlugin interface, API key provider, OIDC provider |
| `capability/` | Resolution engine — NamedList / CapabilityPolicy / SandboxBoundary |
| `channels/` | Outbound notification channel adapters (Epic 18) |
| `circles/` | Circle management, system circle (global knowledge scope) |
| `db/` | PostgreSQL client, migrations, import-on-load for policy files |
| `intercom/` | Centrifugo pub/sub (IntercomService) |
| `lib/` | Shared utilities |
| `mcp/` | MCP registry, protocol client, sera-core MCP server |
| `memory/` | MemoryBlockStore, KnowledgeGitService, EmbeddingService, Qdrant client |
| `metering/` | Token usage tracking, budget enforcement |
| `routes/` | HTTP route handlers |
| `sandbox/` | SandboxManager — Docker container lifecycle |
| `secrets/` | SecretsProvider interface, PostgreSQL AES-256-GCM provider |
| `services/` | Cross-cutting services |
| `skills/` | SkillLibrary, loader, hot-reload |
| `tools/` | Built-in tool implementations (knowledge-store, knowledge-query, etc.) |

## Schema sync rule

JSON Schema files in `schemas/` define the public manifest contract. Any change to a schema file **must** have a corresponding update to the matching `zod` schema in `src/capability/` or `src/agents/`. The import-on-load process (`src/db/import-on-load.ts`) re-imports CapabilityPolicies, SandboxBoundaries, and NamedLists from their YAML directories at startup — a server restart is required to pick up file changes to those resources.

## Testing

- See `docs/TESTING.md` for the full strategy, patterns, and coverage requirements
- Colocate unit tests with source: `src/foo/bar.ts` → `src/foo/bar.test.ts`
- Integration tests (DB, Docker, Centrifugo) live in `src/__tests__/`
- Integration tests require a running database — failures without one are expected and pre-existing
- Always run tests against `core/src/` paths — `core/dist/` contains compiled duplicates

## Docker

- Use **dockerode** for all Docker operations — never shell out to the CLI
- Agent container network: `agent_net`
- SERA-managed container filter label: `sera.sandbox=true`

## Startup order (`src/index.ts`)

The server startup sequence is order-sensitive:

1. Create services (orchestrator, registries, routers)
2. **`initDb()`** — run database migrations (must happen before any DB queries)
3. `orchestrator.startDockerEventListener()` — queries `agent_instances` table
4. Bootstrap Sera agent instance
5. `app.listen()`

Do not reorder steps 2-3 — the Docker event listener will crash if migrations haven't run.

## Learnings

- **`npx` binary resolution is broken**: Always use the full `node_modules/.bin/` path (see Binary paths above).
- **`tsc` without `-p` flag exits 1 and prints help**: Always pass `-p <path-to-tsconfig.json>` explicitly.
- **Running tests from `dist/`**: Vitest targeting `dist/` paths runs compiled output, not source — always target `src/` paths.
- **`vitest` and `@vitest/coverage-v8` must share major versions**: A mismatch (e.g. vitest@2 + coverage-v8@4) causes `npm install` to fail with peer dependency conflicts. Keep them aligned.
- **`node-pg-migrate` CLI flag is `--migrations-dir`** (not `--dir`): The npm script in `package.json` uses this flag. The programmatic API uses `dir` (without the prefix).
- **`node-pg-migrate` column defaults — never wrap in extra single quotes**: For text/boolean/numeric literals use a bare JS string e.g. `default: 'active'`. For SQL expressions use `pgm.func('now()')`. For `text[]` empty-array default use `pgm.func("'{}'")`; for `jsonb` empty-object use `default: '{}'`. Wrapping in extra quotes (e.g. `default: "'{}'"`) causes node-pg-migrate to dollar-quote the string, producing `DEFAULT $pga$'{}'$pga$` which PostgreSQL cannot cast to JSON, resulting in `invalid input syntax for type json`.
- **`simple-git` must be imported as a named export**: `import { simpleGit } from 'simple-git'` — the default import has no call signature and will fail TypeScript compilation. `git.log()` does not accept a raw `string[]` directly via its TypeScript types; cast as `unknown` or use `{ from, to }` options. Note: `{ from: '', to: 'main' }` returns commits *strictly between* the two refs, not the full history — use `['main']` as raw args for all commits on main.
- **`pg-boss` must be imported as a named export**: `import { PgBoss } from 'pg-boss'` — default import fails. The `work()` callback receives `Job<T>[]` (an array), not a single `Job<T>`.
- **`pg-boss` queue names may not contain colons**: Only `[a-zA-Z0-9_\-./]` are allowed. `notification:dispatch` → use `notification.dispatch`. Also always attach `boss.on('error', handler)` before `boss.start()` — unhandled `error` events crash the process with `ERR_UNHANDLED_ERROR`.
- **Qdrant filter with `exactOptionalPropertyTypes`**: Cannot pass `filter: undefined` to `client.search()` — use a conditional: `if (filter) params.filter = filter`. The `vectors_count` field on collection info is absent from the type definition; use `(info as any).points_count` or `(info as any).vectors_count`.
- **`nomic-embed-text` produces 768-dim vectors** (not 1536): The epic spec mentions 1536 for ada-002 compat but the actual model output is 768. Qdrant collections created for Epic 8 namespaces use 768. If operators switch models, they must drop and recreate collections.
- **EmbeddingService singleton in tests**: After `vi.resetModules()`, manually reset the static instance: `(EmbeddingService as any).instance = undefined` — otherwise the stale instance leaks across test files.
- **`jose`'s `createRemoteJWKSet` returns a callable, not a Promise**: Don't call `.catch()` on its return value — it's a `RemoteJWKSetInterface` (a function). Calling it on startup is a valid no-op for early initialisation; actual JWKS fetching happens on first `jwtVerify`. The `cacheMaxAge` option handles kid-mismatch refresh automatically.
- **Mock all db.query calls in tests — including fire-and-forget ones**: When a function calls `db.query(...).catch(() => {})` in a background path (e.g. `UPDATE last_used_at`), the Vitest mock still needs a `mockResolvedValueOnce` for it. Without it the mock returns `undefined` and `.catch()` throws `TypeError: Cannot read properties of undefined`.
- **Auth router splitting pattern**: When a route group has mixed public and protected endpoints under the same prefix, return `{ publicRouter, protectedRouter }` from the factory and mount them separately — `app.use('/api/auth', publicRouter)` and `app.use('/api/auth', authMiddleware, protectedRouter)`. Mounting the same factory call multiple times at the same prefix does not selectively apply middleware.
- **Spec-wrapped vs flat manifest format**: SERA supports two AGENT.yaml formats. Old (flat): `identity` and `model` at top-level, `metadata.tier` as integer. New (spec-wrapped): all agent config inside a `spec` block; `spec.identity`, `spec.model`, `spec.sandboxBoundary`. `AgentManifestLoader` validates both. `IdentityService.generateSystemPrompt` and `ProviderFactory.createFromManifest` resolve from `spec.*` first, then fall back to top-level. Any new service consuming manifests must handle both formats.
- **`GET /api/tools` and `GET /api/templates`**: The web AgentForm calls these two endpoints on load. They are mounted directly in `index.ts` (not behind authMiddleware). `tools` returns `skillRegistry.listAll()`; `templates` delegates to `agentRegistry.listTemplates()`.

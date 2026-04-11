# sera-core

Central API server, LLM proxy, orchestrator, and governance layer. See `docs/ARCHITECTURE.md` → Component Architecture for the full module design and `docs/epics/` for story-level acceptance criteria.

## Tech stack

| Concern        | Library                                                                                                     |
| -------------- | ----------------------------------------------------------------------------------------------------------- |
| Runtime        | Node.js 22 LTS                                                                                              |
| HTTP framework | Express 5 (current) → Fastify v5 (planned — check `docs/ARCHITECTURE.md` → Tech Stack for migration status) |
| Validation     | zod                                                                                                         |
| JWT            | jose v5                                                                                                     |
| Database       | postgres.js + PostgreSQL                                                                                    |
| Job queue      | pg-boss                                                                                                     |
| Docker API     | dockerode                                                                                                   |
| Git operations | simple-git                                                                                                  |
| Embeddings     | Ollama (`nomic-embed-text`)                                                                                 |
| Tests          | Vitest                                                                                                      |

## Binary paths

Use `bunx` to run local binaries:

```bash
# TypeScript — -p flag is required; omitting it causes tsc to print help and exit 1
bunx tsc --noEmit -p D:/projects/homelab/sera/core/tsconfig.json

# Vitest — pass file paths as positional args
bunx vitest run core/src/path/to/file.test.ts

# Run all tests
bunx vitest run
```

## TypeScript strict flags (`core/tsconfig.json`)

Two non-standard flags that cause non-obvious errors:

**`exactOptionalPropertyTypes: true`** — cannot assign `undefined` to an optional property:

```ts
// Wrong
const obj = { foo: maybeUndefined };
// Right
const obj = { ...(maybeUndefined !== undefined ? { foo: maybeUndefined } : {}) };
```

**`noUncheckedIndexedAccess: true`** — array and object index access returns `T | undefined`:

```ts
const item = arr[0]!; // use non-null assertion
const val = map['key']!; // or guard with an if-check
```

## Source module map (`src/`)

| Directory     | Purpose                                                                |
| ------------- | ---------------------------------------------------------------------- |
| `agents/`     | AgentFactory, instance management, Sera bootstrap                      |
| `audit/`      | Merkle hash-chain audit trail                                          |
| `auth/`       | AuthPlugin interface, API key provider, OIDC provider                  |
| `capability/` | Resolution engine — NamedList / CapabilityPolicy / SandboxBoundary     |
| `channels/`   | Outbound notification channel adapters (Epic 18)                       |
| `circles/`    | Circle management, system circle (global knowledge scope)              |
| `db/`         | PostgreSQL client, migrations, import-on-load for policy files         |
| `intercom/`   | Centrifugo pub/sub (IntercomService)                                   |
| `lib/`        | Shared utilities                                                       |
| `mcp/`        | MCP registry, protocol client, sera-core MCP server                    |
| `memory/`     | MemoryBlockStore, KnowledgeGitService, EmbeddingService, Qdrant client |
| `metering/`   | Token usage tracking, budget enforcement                               |
| `routes/`     | HTTP route handlers                                                    |
| `sandbox/`    | SandboxManager — Docker container lifecycle                            |
| `secrets/`    | SecretsProvider interface, PostgreSQL AES-256-GCM provider             |
| `services/`   | Cross-cutting services                                                 |
| `skills/`     | SkillLibrary, loader, hot-reload                                       |
| `tools/`      | Built-in tool implementations (knowledge-store, knowledge-query, etc.) |

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

- **Express 5 does not match `router.get('/')` on mounted sub-routers**: When a router is mounted via `app.use('/api/foo', router)`, Express 5 does NOT match `router.get('/')` for `GET /api/foo`. Sub-paths like `router.get('/bar')` match fine. Workaround: use a named path like `router.get('/list')` instead of `/`. This was not an issue in Express 4.
- **Stale `dist/` directory can shadow source in dev mode**: When running `tsx watch src/index.ts`, if a `dist/` directory exists with compiled `.js` files, tsx may resolve `import './foo.js'` to the dist version instead of the source `.ts`. Delete `dist/` in the container during dev: `docker exec sera-core rm -rf /app/dist`.
- **Legacy `config.ts` route conflict**: The `createConfigRouter()` mounted at `app.use('/api', ...)` had `router.get('/providers')` which resolved to `/api/providers` and shadowed the dedicated `createProvidersRouter` at `app.use('/api/providers', ...)`. Always check for route prefix collisions when mounting routers at different levels.
- **Build uses tsup (esbuild-based)**: `bun run build` runs tsup for fast file-per-file transpilation (~100ms). Type checking is separate via `tsc --noEmit`. Config in `tsup.config.ts`.
- **`tsx watch` does not detect file changes inside Docker on Windows**: Same as the Vite HMR issue — Docker Desktop volume mounts don't propagate inotify events. Use `docker restart sera-core` to pick up source changes during dev.
- **`tsc` without `-p` flag exits 1 and prints help**: Always pass `-p <path-to-tsconfig.json>` explicitly.
- **Running tests from `dist/`**: Vitest targeting `dist/` paths runs compiled output, not source — always target `src/` paths.
- **`vitest` and `@vitest/coverage-v8` must share major versions**: A mismatch (e.g. vitest@2 + coverage-v8@4) causes `bun install` to fail with peer dependency conflicts. Keep them aligned.
- **`node-pg-migrate` CLI flag is `--migrations-dir`** (not `--dir`): The npm script in `package.json` uses this flag. The programmatic API uses `dir` (without the prefix).
- **`node-pg-migrate` column defaults — never wrap in extra single quotes**: For text/boolean/numeric literals use a bare JS string e.g. `default: 'active'`. For SQL expressions use `pgm.func('now()')`. For `text[]` empty-array default use `pgm.func("'{}'")`; for `jsonb` empty-object use `default: '{}'`. Wrapping in extra quotes (e.g. `default: "'{}'"`) causes node-pg-migrate to dollar-quote the string, producing `DEFAULT $pga$'{}'$pga$` which PostgreSQL cannot cast to JSON, resulting in `invalid input syntax for type json`.
- **`simple-git` must be imported as a named export**: `import { simpleGit } from 'simple-git'` — the default import has no call signature and will fail TypeScript compilation. `git.log()` does not accept a raw `string[]` directly via its TypeScript types; cast as `unknown` or use `{ from, to }` options. Note: `{ from: '', to: 'main' }` returns commits _strictly between_ the two refs, not the full history — use `['main']` as raw args for all commits on main.
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
- **Agent-runtime runs on bun**: `core/sandbox/Dockerfile.worker` uses `oven/bun:1-slim` as base image. No TypeScript build step — bun runs `.ts` files directly. Faster cold start and smaller image than Node.js.
- **LiteLLM replaced by `@mariozechner/pi-ai` (in-process routing)**: LLM calls happen in-process via `LlmRouter` → `ProviderRegistry` → pi-mono provider functions. Provider config lives in `core/config/providers.json`. Cloud providers are auto-detected by model name; local providers (LM Studio, Ollama) are registered in `providers.json` with a `baseUrl`. `LLM_BASE_URL` + `LLM_MODEL` env vars bootstrap a single default provider without a config file.
- **pi-mono `Model<TApi>` has all fields required**: `id`, `name`, `api`, `provider`, `baseUrl`, `reasoning`, `input`, `cost`, `contextWindow`, `maxTokens` are all non-optional. Provide sensible defaults (`''` for baseUrl, `false` for reasoning, `['text']` for input, zero cost, 128k context).
- **Centrifugo v6 health endpoint — use `health.enabled: true`**: The `/health` HTTP endpoint is disabled by default. The wrong key (`health_check.enable`) silently has no effect. Core health check treats 404 as `degraded`.
- **Centrifugo v6 config — `hmac_secret_key` moved under `client.token`**: In v6+ the JWT HMAC secret must be at `client.token.hmac_secret_key`, NOT top-level `token.hmac_secret_key`. A misplaced key produces `"disabled JWT algorithm: HS256"` errors. The `CENTRIFUGO_TOKEN_SECRET` env var in `IntercomService` must match.
- **Agent LLM routing — `LlmRouterProvider` for YAML-loaded agents**: Model name in `spec.model.name` must match a `modelName` entry in `core/config/providers.json`. `LlmRouter` is injected via `ProviderFactory.createFromManifest(manifest, router)` and `AgentFactory.createAgent(manifest, id, intercom, router)`.
- **Qwen3 thinking models produce `reasoning_content` in streaming**: Pi-mono maps these to `thinking_delta` events. `LlmRouterProvider` maps `thinking_delta` to `LLMStreamChunk.reasoning`. Thinking phase can take 2+ minutes for 35B models before content arrives.
- **Context window config is per-model in providers.json**: `contextWindow`, `maxTokens`, `contextStrategy`, `contextHighWaterMark` are optional fields. LlmRouter.buildModel() defaults: 128K context, 4K max tokens. Agent-runtime has its own `ContextManager` — separate from core-side config.
- **Reasoning/thinking models need `reasoning: true` in providers.json**: Pi-mono's `Model.reasoning` flag must be `true` for models that emit `reasoning_content` (Qwen3, DeepSeek-R1, o1/o3). `LlmRouter.buildModel()` auto-detects by name pattern, but explicit config is more reliable. See #406/#403.
- **Audit API returns snake_case, frontend types are camelCase**: `getAuditEvents()` API wrapper maps between them. New audit fields must be added to both the type and the mapping.
- **Metering API only accepts `groupBy=hour|day`**: `/api/metering/usage` rejects `groupBy=agent` with 400. Agent-level aggregation must be done client-side. See #399.
- **Embedded MCP server must include auth headers for internal HTTP calls**: `SeraMCPServer.handleChat()` calls `/api/chat` via HTTP — must include `Authorization: Bearer <key>`. See #411.
- **Empty LLM responses produce misleading output**: When the LLM errors (e.g., context overflow), `WorkerAgent.process()` now detects empty responses and surfaces meaningful error messages. See #410.
- **Schedule `agent_name` column may be null**: GET /api/schedules JOINs `agent_instances` to resolve the name. Frontend sanitizes raw SQL errors in `lastRunOutput`. See #407.
- **Agent container chatUrl must use sera_net IP, not agent_net**: sera-core is only on `sera_net`. The `chatUrl` must use the `sera_net` IP. `containerIp` (agent_net) is kept separately for egress ACL mapping. See #428.
- **Agent-runtime image must be rebuilt after code changes**: `sera-agent-worker:latest` bakes in `core/agent-runtime/src/` at build time — NOT bind-mounted in dev. Rebuild with `docker build -f core/sandbox/Dockerfile.worker -t sera-agent-worker:latest core/`.
- **Chat routes are container-only (no in-process fallback)**: Since #428, `POST /api/chat` routes exclusively through the agent container's chat server. Returns 503 if container unavailable. `WorkerAgent.process()` and `BaseAgent.processStream()` are still used by channel adapters, process flows, and `openai-compat.ts`.
- **Cloud API keys are ingested into the secrets store on first startup**: `ProviderRegistry.ingestEnvKeys()` persists env vars to the encrypted PostgreSQL secrets store on first boot. Both `GOOGLE_API_KEY` and `GEMINI_API_KEY` are accepted for Google. See #404.
- **Context window config flows from core to agent-runtime via env**: `Orchestrator.startInstance()` passes `CONTEXT_WINDOW` and `CONTEXT_COMPACTION_STRATEGY` env vars to the container. Agent-runtime reads these (priority: env → hardcoded → default 32K). See #453.
- **Ephemeral agents cannot use the task queue**: `task_queue` endpoints reject ephemeral agents with 405. Use `spawn-ephemeral` action instead. See #334.
- **Dynamic provider model names cause LM Studio JIT conflicts**: `DynamicProviderManager` names like `dp-agw-qwen/qwen3.5-35b-a3b` don't match LM Studio's loaded model, causing JIT-load with default 4096 context. Use static `providers.json` entry names instead. See #497.
- **Schedule task column is JSONB — plain strings must be wrapped**: Plain prompt strings must be wrapped as `{"prompt": "..."}` before insertion. `ScheduleService.createSchedule()` handles this automatically. See #599/#605.
- **pg-boss v9+ requires `createQueue()` before `schedule()`**: `ScheduleService.ensureQueueAndSchedule()` wraps both calls. The `agent-schedule` worker queue is separate from per-schedule queues.
- **Token budget `0` = unlimited**: `MeteringService.checkBudget()` treats quota `0` as unlimited. PATCH `/api/budget/agents/:id/budget` accepts `0` or `null`. Defaults: 100K hourly, 1M daily.
- **Schedule task JSONB→TEXT mismatch on trigger**: `schedules.task` is JSONB but `task_queue.task` is TEXT. `triggerSchedule()` uses `resolveTaskPrompt()` to extract the prompt string before enqueuing.
- **AgentTemplate Zod schema strips unknown fields**: New template spec fields must be added to both `types.ts` AND `schemas.ts`. See #603.
- **Templates are reimported on every startup**: `ResourceImporter.importAll()` syncs manifest schedules — changed schedules updated in-place, removed deleted, operator-created API schedules never touched. See #601/#602.

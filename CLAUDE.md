# SERA — Project Instructions

## What is SERA

SERA (Sandboxed Extensible Reasoning Agent) is a Docker-native multi-agent AI orchestration platform. Read `docs/ARCHITECTURE.md` before implementing anything — do not infer architecture from the existing codebase alone; implementation may be ahead of or behind the canonical spec.

## Key documentation

Load selectively based on your task — do not load everything upfront:

| Document                       | Load when                                                                         |
| ------------------------------ | --------------------------------------------------------------------------------- |
| `docs/ARCHITECTURE.md`         | Any implementation task — canonical tech stack, data models, all design decisions |
| `docs/IMPLEMENTATION-ORDER.md` | Starting an epic — epic dependencies and build sequence                           |
| `docs/TESTING.md`              | Writing tests — strategy, patterns, coverage requirements                         |
| `docs/epics/{n}-{name}.md`     | Implementing stories — acceptance criteria and DB schema for that epic            |
| `docs/openapi.yaml`            | Adding or modifying API endpoints — path-level spec for all ~190 endpoints        |
| `docs/AGENT-WORKFLOW.md`       | Multi-agent coordination — agent roles, issue flow, validation loops              |

## Environment

- **Platform:** Windows 11 / bash shell — use Unix syntax (forward slashes) throughout
- **Working directory:** `D:/projects/homelab/sera`
- **Package manager:** bun workspaces (`core/` and `web/` are workspace packages)
- **`cd` does not persist between shell calls** — use absolute paths in every command

## Codebase map

```
sera/
  core/                  # sera-core API server        → see core/CLAUDE.md
  core/agent-runtime/    # Agent worker process         → see core/agent-runtime/CLAUDE.md
  web/                   # sera-web dashboard           → see web/CLAUDE.md
  tui/                   # Go terminal UI               → see tui/CLAUDE.md
  docs/                  # Architecture and epic specs  (load on demand — see table above)
  agents/                # Agent YAML manifests (instances)
  templates/             # AgentTemplate definitions
  schemas/               # JSON Schema for manifests and policies
  sandbox-boundaries/    # Tier policy definitions (tier-1/2/3.yaml)
  capability-policies/   # CapabilityPolicy definitions
  lists/                 # Network and command allow/denylists
  circles/               # Circle definitions and shared memory
  litellm/               # LiteLLM provider gateway config
  centrifugo/            # Centrifugo real-time messaging config
```

## Code Quality

- **No `any` types** unless absolutely necessary
- **Check `node_modules` for external API type definitions** instead of guessing
- **Never use inline imports** — no `await import("./foo.js")`, no `import("pkg").Type` in type positions, no dynamic imports for types; always use standard top-level imports
- **Never remove or downgrade code to fix type errors from outdated dependencies** — upgrade the dependency instead
- **Always ask before removing functionality** or code that appears to be intentional
- **Never hardcode keybinding checks** (e.g. `matchesKey(keyData, "ctrl+x")`) — all keybindings must be configurable with a default in the matching defaults object (`DEFAULT_EDITOR_KEYBINDINGS` or `DEFAULT_APP_KEYBINDINGS`)

## CLAUDE.md conventions

- Each subdirectory with distinct tooling, language, or behaviour has its own `CLAUDE.md`
- **If you begin working in a large subdirectory that does not yet have a `CLAUDE.md`, create one before writing implementation code** — cover the local tooling, binary paths, and any known gotchas
- Keep all CLAUDE.md files short — reference `docs/` files rather than duplicating content
- The top-level CLAUDE.md covers cross-cutting environment concerns; subdirectory files cover specifics

## Learnings protocol

When you discover a non-obvious environment behaviour, fix a recurring error, or make a significant implementation decision not fully covered by the docs, add it to the **Learnings** section of the most relevant CLAUDE.md. This prevents future sessions from repeating the same discovery.

Format:

```
- **[Short title]**: What the issue was and what the resolution or decision is.
```

Only record durable facts — environment quirks, library gotchas, architectural decisions made during implementation. Not task-specific notes.

## Memory protocol

Claude Code's auto-memory system persists across conversations at `~/.claude/projects/<project>/memory/`. Use it for:

- **User preferences** (role, coding style, communication style)
- **Feedback** (corrections, confirmed approaches — what to repeat or avoid)
- **Project context** (ongoing initiatives, deadlines, decisions not in code/docs)
- **External references** (where to find things outside the repo)

Do **not** duplicate CLAUDE.md learnings into memory — learnings belong in CLAUDE.md (checked into git, shared with all contributors), while memory is personal to the Claude Code instance.

When completing a workflow loop or resolving a non-trivial issue, check whether a new learning should be added to the relevant CLAUDE.md and/or a memory should be saved.

## Docker Compose (dev)

- **Dev start command:** `docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d`
- **Dev entrypoints:** `core/docker-entrypoint.dev.sh` and `web/docker-entrypoint.dev.sh` run `bun install` into the named volume on first boot — do not remove these
- **Named volumes for node_modules:** The dev compose shadows `/app/node_modules` with named volumes (`node_modules_core`, `node_modules_web`). These start empty; the entrypoint scripts populate them. To force a fresh install: `docker compose ... down -v` then `up -d`
- **Migrations run automatically:** `initDb()` in `core/src/index.ts` runs `node-pg-migrate` on startup — no manual migration step needed
- **Shell scripts must use LF line endings:** Any `.sh` file mounted into a Linux container will break with CRLF. After creating shell scripts, run `sed -i 's/\r$//'` on them

## Learnings

- **`bunx` replaces `npx`**: bun is the project package manager. Use `bunx` to run local binaries (e.g. `bunx vitest run`, `bunx tsc --noEmit`). The old `npx` and `node_modules/.bin/` shim workarounds are no longer needed.
- **`cd` does not persist between shell calls**: Every Bash tool call starts in the default working directory — always use absolute paths.
- **Git Bash mangles absolute Linux paths in `docker exec`**: Prefix with `MSYS_NO_PATHCONV=1` when passing paths like `/app/...` to `docker exec`.
- **Dev dependency version alignment**: `vitest` and `@vitest/coverage-v8` must share the same major version — a mismatch breaks `bun install` in Docker builds.
- **Core build uses tsup (esbuild)**: `core/package.json` build script runs `tsup` for fast file-per-file transpilation (~100ms). Type checking is separate via `tsc --noEmit`. The tsup config is in `core/tsup.config.ts`.
- **Agent-runtime runs on bun**: `core/sandbox/Dockerfile.worker` uses `oven/bun:1-slim` as base image. No TypeScript build step — bun runs `.ts` files directly. Faster cold start and smaller image than Node.js.
- **`simple-git` and `pg-boss` use named exports**: `import { simpleGit } from 'simple-git'` and `import { PgBoss } from 'pg-boss'` — default imports have no call signatures and fail tsc. See `core/CLAUDE.md` for further gotchas with each library.
- **LiteLLM replaced by `@mariozechner/pi-ai` (in-process routing)**: The `litellm` sidecar container is gone. LLM calls now happen in-process via `LlmRouter` → `ProviderRegistry` → pi-mono provider functions. Provider config lives in `core/config/providers.json`. Cloud providers (gpt-_, claude-_, gemini-\*) are auto-detected by model name and read their API keys from standard env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, …). Local providers (LM Studio, Ollama) are registered in `providers.json` with a `baseUrl`. `LLM_BASE_URL` + `LLM_MODEL` env vars bootstrap a single default provider without a config file. See `core/src/llm/ProviderRegistry.ts` and `core/src/llm/LlmRouter.ts`.
- **pi-mono `Model<TApi>` has all fields required**: All of `id`, `name`, `api`, `provider`, `baseUrl`, `reasoning`, `input`, `cost`, `contextWindow`, `maxTokens` are non-optional. When constructing a model dynamically, provide sensible defaults (`''` for baseUrl, `false` for reasoning, `['text']` for input, zero cost, 128k context).
- **Centrifugo v6 health endpoint — use `health.enabled: true`**: The `/health` HTTP endpoint is disabled by default. Enable it with `"health": { "enabled": true }` in `centrifugo/config.json`. The wrong key (`health_check.enable`) silently has no effect and the endpoint returns 404, which the core health check treats as `degraded`.
- **Centrifugo v6 config — `hmac_secret_key` moved under `client.token`**: In Centrifugo v6+, the JWT HMAC secret must be at `client.token.hmac_secret_key`, NOT the top-level `token.hmac_secret_key` path used in v5. A misplaced key produces `"unknown key in configuration file"` and `"disabled JWT algorithm: HS256"` errors causing all web-client connections to fail. The `CENTRIFUGO_TOKEN_SECRET` env var (defaults to `'sera-token-secret'`) in `IntercomService` must match this value.
- **Agent LLM routing — `LlmRouterProvider` for YAML-loaded agents**: YAML-loaded agents now use `LlmRouterProvider` (backed by `LlmRouter` → pi-mono) instead of the legacy `OpenAIProvider`. The model name in `spec.model.name` must match a `modelName` entry in `core/config/providers.json`. The `providers.json` must be updated when adding new local models. `LlmRouter` is injected via `ProviderFactory.createFromManifest(manifest, router)` and `AgentFactory.createAgent(manifest, id, intercom, router)`.
- **Qwen3 thinking models produce `reasoning_content` in streaming**: `qwen3.5-35b-a3b` and similar models emit `reasoning_content` (not `content`) in SSE deltas during their thinking phase. Pi-mono maps these to `thinking_delta` events. `LlmRouterProvider` maps `thinking_delta` to `LLMStreamChunk.reasoning`. The thinking phase can generate hundreds of tokens before any actual `content` arrives, making responses take 2+ minutes for a 35B model.
- **Squid egress proxy fails on `docker restart`**: The squid PID file (`/var/run/squid/squid.pid`) persists across restarts, causing `FATAL: Squid is already running`. Workaround: `docker compose down sera-egress-proxy && docker compose up -d sera-egress-proxy`. See #363.
- **sera-web healthcheck reports unhealthy but UI works**: The `wget` command in the healthcheck can't connect to `localhost` inside the container, even though Vite is listening on `0.0.0.0:5173`. `node -e "fetch(...)"` works. See #364.
- **API endpoints require auth header**: All `/api/*` endpoints (except `/api/health/*`) require `Authorization: Bearer <key>`. Dev key: `sera_bootstrap_dev_123`. The `runtime-verify.sh` script must include this header.
- **Providers endpoint is `/api/providers/list` not `/api/providers`**: Express 5 doesn't match `router.get('/')` on mounted sub-routers. The providers router uses `router.get('/list')`. This is documented in `core/CLAUDE.md` but easy to forget in scripts and tests.
- **Context window config is per-model in providers.json**: `contextWindow`, `maxTokens`, `contextStrategy`, `contextHighWaterMark` are optional fields on each provider entry. LlmRouter.buildModel() reads them (defaults: 128K context, 4K max tokens). The agent-runtime container has its own `ContextManager` with `MODEL_CONTEXT_WINDOWS` lookup and `CONTEXT_COMPACTION_STRATEGY` env var — these are separate from the core-side config and apply only to container-based agents.
- **web/bun.lock must be generated in standalone Docker context**: The web Dockerfile builds with `context: ./web` (not the workspace root). Running `bun install` locally inside the workspace produces a different lockfile than running it standalone with only `web/package.json`. To regenerate correctly: `MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd)/web:/app" -w /app oven/bun:1-alpine bun install`. Never run `bun install` via Docker volume mount into the host `web/` directory — it contaminates `node_modules` with Linux binaries (e.g. esbuild) that crash on Windows.
- **Docker volume mount + `bun install` contaminates host node_modules**: Running `bun install` inside a Docker container with the host's `web/` bind-mounted replaces platform-specific binaries (esbuild, etc.) with Linux versions. This causes `Host version "X" does not match binary version "Y"` errors. Fix: `rm -rf web/node_modules && bun install` from the host.
- **Reasoning/thinking models need `reasoning: true` in providers.json**: Pi-mono's `Model.reasoning` flag must be `true` for models that emit `reasoning_content` (Qwen3, DeepSeek-R1, o1/o3). Without it, the thinking→content transition isn't handled correctly and response content is lost. `LlmRouter.buildModel()` now auto-detects by model name pattern, but explicit `"reasoning": true` in providers.json is more reliable. See #406/#403.
- **Audit API returns snake_case, frontend types are camelCase**: The PostgreSQL `audit_trail` table uses snake_case columns (`actor_id`, `event_type`, `resource_type`). The `AuditEvent` TypeScript type uses camelCase. The `getAuditEvents()` API wrapper maps between them. Any new audit fields must be added to both the type and the mapping.
- **Metering API only accepts `groupBy=hour|day`**: The `/api/metering/usage` endpoint rejects `groupBy=agent` with a 400. Agent-level aggregation must be done client-side from the day-grouped rows. See #399.
- **Embedded MCP server must include auth headers for internal HTTP calls**: `SeraMCPServer.handleChat()` calls `/api/chat` via HTTP — must include `Authorization: Bearer <key>` since all `/api/*` routes are behind `authMiddleware`. Without it, requests fail silently or return unexpected results. See #411.
- **Empty LLM responses produce misleading 'Completed task' output**: When the LLM errors (e.g., context window overflow), `WorkerAgent.process()` returned `'Completed task: <input>'` with empty `finalAnswer`. Fixed to detect empty responses and surface meaningful error messages. See #410.
- **Schedule `agent_name` column may be null**: The `schedules` table stores `agent_instance_id` but `agent_name` can be null. The GET /api/schedules route now JOINs `agent_instances` to resolve the name. Frontend also sanitizes raw SQL errors in `lastRunOutput`. See #407.
- **Agent container chatUrl must use sera_net IP, not agent_net**: sera-core is only on `sera_net`. Agent containers are on both `agent_net` (for egress proxy routing) and `sera_net` (for reaching sera-core). The `chatUrl` must use the `sera_net` IP so core can reach the container's chat server. `containerIp` (agent_net) is kept separately for egress ACL mapping. See #428.
- **Agent-runtime image must be rebuilt after agent-runtime code changes**: The `sera-agent-worker:latest` image bakes in `core/agent-runtime/src/` at build time — it is NOT bind-mounted in dev mode. After editing files in `core/agent-runtime/`, rebuild with `docker build -f core/sandbox/Dockerfile.worker -t sera-agent-worker:latest core/`. The `core/agent-runtime/bun.lock` must also be regenerated in Linux context if dependencies changed: `MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd)/core/agent-runtime:/app" -w /app oven/bun:1-alpine bun install`.
- **Chat routes are container-only (no in-process fallback)**: Since #428, `POST /api/chat` and `/api/chat/stream` route exclusively through the agent container's chat server. There is no fallback to `WorkerAgent.process()` or `BaseAgent.processStream()`. If the container is unavailable, the route returns 503. `SandboxManager.waitForChatReady()` polls the container's `/health` endpoint with exponential backoff before marking it ready. `WorkerAgent.process()` and `BaseAgent.processStream()` are still used by channel adapters, process flows, and `openai-compat.ts`.
- **Cloud API keys are ingested into the secrets store on first startup**: Env vars like `OPENAI_API_KEY`, `GOOGLE_API_KEY`, `GEMINI_API_KEY` in `docker-compose.yaml` are a one-time ingestion path. `ProviderRegistry.ingestEnvKeys()` persists them to the encrypted PostgreSQL secrets store on first boot. After that the key survives container rebuilds without the env var. Both `GOOGLE_API_KEY` and `GEMINI_API_KEY` are accepted for the Google provider. See #404.
- **Context window config flows from core to agent-runtime via env**: `Orchestrator.startInstance()` resolves the model's `contextWindow` and `contextStrategy` from `ProviderRegistry` and passes them as `CONTEXT_WINDOW` and `CONTEXT_COMPACTION_STRATEGY` env vars to the container. The agent-runtime `ContextManager` reads these (priority: env var → hardcoded `MODEL_CONTEXT_WINDOWS` → default 32K). No need to update the hardcoded table when adding new models to `providers.json`. See #453.
- **Ephemeral agents cannot use the task queue**: The `task_queue` endpoints and `delegate-task` skill's `send` action reject ephemeral agents with 405. Use the `spawn-ephemeral` action instead, which creates+spawns+executes+returns in one call. See #334.
- **Prettier format check differs between Windows and Linux**: Jules PRs formatted on Linux may fail CI format check when our pre-commit hook reformats on Windows. The `ChatMessageBubble.tsx` multi-line vs single-line className pattern is a known divergence. Always run `bun run format` before pushing.
- **Dynamic provider model names cause LM Studio JIT conflicts**: Models discovered by `DynamicProviderManager` get `dp-{providerId}-{modelId}` names (e.g., `dp-agw-qwen/qwen3.5-35b-a3b`). When sent to LM Studio, this name doesn't match the already-loaded model, causing LM Studio to JIT-load a second instance with default 4096 context. Use the static `providers.json` entry name instead. See #497.
- **Schedule task column is JSONB — plain strings must be wrapped**: The `task` column in the `schedules` table is JSONB. Plain prompt strings (e.g. from template YAML) must be wrapped as `{"prompt": "..."}` before insertion. `ScheduleService.createSchedule()` now handles this automatically. The `schedule-task` skill also normalizes (line 68-73). See #599/#605.
- **Schedule task JSONB→TEXT mismatch on trigger**: `schedules.task` is JSONB (auto-deserialized by PostgreSQL on SELECT), but `task_queue.task` is TEXT. Passing the deserialized object directly to an INSERT produces `"[object Object]"`. `triggerSchedule()` now uses `resolveTaskPrompt()` to extract the prompt string from `{prompt: "..."}` before enqueuing. Same applies to ephemeral agent `startInstance()` calls.
- **AgentTemplate Zod schema strips unknown fields**: The `AgentTemplateSchema` in `schemas.ts` uses Zod's default strict parsing. Any field not in the schema is silently dropped during template import. When adding new template spec fields (e.g. `schedules`), always add them to both `types.ts` AND `schemas.ts`. See #603.
- **Templates are reimported on every startup**: `ResourceImporter.importAll()` runs in `index.ts` after `initDb()`. `upsertTemplate()` syncs manifest schedules to all existing instances via `upsertManifestSchedule()` — changed schedules are updated in-place, removed schedules are deleted, and operator-created API schedules are never touched. See #601/#602.

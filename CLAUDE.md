# SERA — Project Instructions

## What is SERA

SERA (Sandboxed Extensible Reasoning Agent) is a Docker-native multi-agent AI orchestration platform. Read `docs/ARCHITECTURE.md` before implementing anything — do not infer architecture from the existing codebase alone; implementation may be ahead of or behind the canonical spec.

## Key documentation

Load selectively based on your task — do not load everything upfront:

| Document | Load when |
|---|---|
| `docs/ARCHITECTURE.md` | Any implementation task — canonical tech stack, data models, all design decisions |
| `docs/IMPLEMENTATION-ORDER.md` | Starting an epic — epic dependencies and build sequence |
| `docs/TESTING.md` | Writing tests — strategy, patterns, coverage requirements |
| `docs/epics/{n}-{name}.md` | Implementing stories — acceptance criteria and DB schema for that epic |
| `docs/openapi.yaml` | Adding or modifying API endpoints — path-level spec for all ~190 endpoints |
| `docs/AGENT-WORKFLOW.md` | Multi-agent coordination — agent roles, issue flow, validation loops |

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
- **LiteLLM replaced by `@mariozechner/pi-ai` (in-process routing)**: The `litellm` sidecar container is gone. LLM calls now happen in-process via `LlmRouter` → `ProviderRegistry` → pi-mono provider functions. Provider config lives in `core/config/providers.json`. Cloud providers (gpt-*, claude-*, gemini-*) are auto-detected by model name and read their API keys from standard env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, …). Local providers (LM Studio, Ollama) are registered in `providers.json` with a `baseUrl`. `LLM_BASE_URL` + `LLM_MODEL` env vars bootstrap a single default provider without a config file. See `core/src/llm/ProviderRegistry.ts` and `core/src/llm/LlmRouter.ts`.
- **pi-mono `Model<TApi>` has all fields required**: All of `id`, `name`, `api`, `provider`, `baseUrl`, `reasoning`, `input`, `cost`, `contextWindow`, `maxTokens` are non-optional. When constructing a model dynamically, provide sensible defaults (`''` for baseUrl, `false` for reasoning, `['text']` for input, zero cost, 128k context).
- **Centrifugo v6 health endpoint — use `health.enabled: true`**: The `/health` HTTP endpoint is disabled by default. Enable it with `"health": { "enabled": true }` in `centrifugo/config.json`. The wrong key (`health_check.enable`) silently has no effect and the endpoint returns 404, which the core health check treats as `degraded`.
- **Centrifugo v6 config — `hmac_secret_key` moved under `client.token`**: In Centrifugo v6+, the JWT HMAC secret must be at `client.token.hmac_secret_key`, NOT the top-level `token.hmac_secret_key` path used in v5. A misplaced key produces `"unknown key in configuration file"` and `"disabled JWT algorithm: HS256"` errors causing all web-client connections to fail. The `CENTRIFUGO_TOKEN_SECRET` env var (defaults to `'sera-token-secret'`) in `IntercomService` must match this value.
- **Agent LLM routing — `LlmRouterProvider` for YAML-loaded agents**: YAML-loaded agents now use `LlmRouterProvider` (backed by `LlmRouter` → pi-mono) instead of the legacy `OpenAIProvider`. The model name in `spec.model.name` must match a `modelName` entry in `core/config/providers.json`. The `providers.json` must be updated when adding new local models. `LlmRouter` is injected via `ProviderFactory.createFromManifest(manifest, router)` and `AgentFactory.createAgent(manifest, id, intercom, router)`.
- **Qwen3 thinking models produce `reasoning_content` in streaming**: `qwen3.5-35b-a3b` and similar models emit `reasoning_content` (not `content`) in SSE deltas during their thinking phase. Pi-mono maps these to `thinking_delta` events. `LlmRouterProvider` maps `thinking_delta` to `LLMStreamChunk.reasoning`. The thinking phase can generate hundreds of tokens before any actual `content` arrives, making responses take 2+ minutes for a 35B model.

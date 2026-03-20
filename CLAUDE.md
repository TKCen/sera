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
| `docs/openapi.yaml` | Adding or modifying API endpoints |
| `docs/API_SCHEMAS.md` | Working with request/response shapes |

## Environment

- **Platform:** Windows 11 / bash shell — use Unix syntax (forward slashes) throughout
- **Working directory:** `D:/projects/homelab/sera`
- **Package manager:** npm workspaces (`core/` and `web/` are workspace packages)
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
- **Dev entrypoints:** `core/docker-entrypoint.dev.sh` and `web/docker-entrypoint.dev.sh` run `npm install` into the named volume on first boot — do not remove these
- **Named volumes for node_modules:** The dev compose shadows `/app/node_modules` with named volumes (`node_modules_core`, `node_modules_web`). These start empty; the entrypoint scripts populate them. To force a fresh install: `docker compose ... down -v` then `up -d`
- **Migrations run automatically:** `initDb()` in `core/src/index.ts` runs `node-pg-migrate` on startup — no manual migration step needed
- **Shell scripts must use LF line endings:** Any `.sh` file mounted into a Linux container will break with CRLF. After creating shell scripts, run `sed -i 's/\r$//'` on them

## Learnings

- **`npx` and `node_modules/.bin/` shims both fail in Git Bash**: The `.bin/` shims are bash scripts that Git Bash can't execute (`SyntaxError: missing ) after argument list`). Use the underlying Node entry points directly — e.g. `node web/node_modules/typescript/bin/tsc` and `node web/node_modules/vitest/vitest.mjs`. Always run from the workspace root with full paths.
- **`cd` does not persist between shell calls**: Every Bash tool call starts in the default working directory — always use absolute paths.
- **Git Bash mangles absolute Linux paths in `docker exec`**: Prefix with `MSYS_NO_PATHCONV=1` when passing paths like `/app/...` to `docker exec`.
- **Dev dependency version alignment**: `vitest` and `@vitest/coverage-v8` must share the same major version — a mismatch breaks `npm install` in Docker builds.
- **`simple-git` and `pg-boss` use named exports**: `import { simpleGit } from 'simple-git'` and `import { PgBoss } from 'pg-boss'` — default imports have no call signatures and fail tsc. See `core/CLAUDE.md` for further gotchas with each library.
- **LiteLLM replaced by `@mariozechner/pi-ai` (in-process routing)**: The `litellm` sidecar container is gone. LLM calls now happen in-process via `LlmRouter` → `ProviderRegistry` → pi-mono provider functions. Provider config lives in `core/config/providers.json`. Cloud providers (gpt-*, claude-*, gemini-*) are auto-detected by model name and read their API keys from standard env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, …). Local providers (LM Studio, Ollama) are registered in `providers.json` with a `baseUrl`. `LLM_BASE_URL` + `LLM_MODEL` env vars bootstrap a single default provider without a config file. See `core/src/llm/ProviderRegistry.ts` and `core/src/llm/LlmRouter.ts`.
- **pi-mono `Model<TApi>` has all fields required**: All of `id`, `name`, `api`, `provider`, `baseUrl`, `reasoning`, `input`, `cost`, `contextWindow`, `maxTokens` are non-optional. When constructing a model dynamically, provide sensible defaults (`''` for baseUrl, `false` for reasoning, `['text']` for input, zero cost, 128k context).

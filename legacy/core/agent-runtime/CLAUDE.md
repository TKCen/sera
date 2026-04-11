# sera-core / agent-runtime

Lightweight TypeScript process that runs **inside each agent container** on the **bun runtime**. It is not a copy of sera-core — it is a minimal reasoning loop purpose-built for the sandboxed environment.

## Key distinctions from sera-core

- Reads the task from **stdin** (JSON) and writes the result to **stdout** — no HTTP server
- All LLM calls go to `sera-core /v1/llm/chat/completions` — never directly to LiteLLM or an upstream provider
- Publishes thoughts to Centrifugo via the HTTP publish API — does not use IntercomService
- Has no direct database access — all persistent state lives in sera-core's DB and the workspace bind mount
- Runs as an unprivileged user inside the container
- **Runtime is bun, not Node.js** — bun runs TypeScript directly, no build step needed

## Architecture reference

Before modifying the reasoning loop, context assembly, or graceful shutdown behaviour, read:
- `docs/ARCHITECTURE.md` → Component Architecture → agent-runtime
- `docs/epics/05-agent-runtime.md` — full acceptance criteria for the reasoning loop, context window management, task queue integration, and graceful shutdown

## Binary paths

This package has its own `node_modules`. Use `bunx` to run local binaries:

```bash
# TypeScript typecheck
bunx tsc --noEmit -p D:/projects/homelab/sera/core/agent-runtime/tsconfig.json

# Vitest
bunx vitest run

# Run directly (bun runs TS natively)
bun src/index.ts
```

## Container image

The worker image is built from `core/sandbox/Dockerfile.worker` using `oven/bun:1-slim` as the base image. No TypeScript compilation step — bun runs `.ts` source directly.

After modifying the runtime source, rebuild:

```bash
docker build -f D:/projects/homelab/sera/core/sandbox/Dockerfile.worker -t sera-agent-worker:latest D:/projects/homelab/sera/core
```

The same TypeScript strict flags from `core/tsconfig.json` apply here — see `core/CLAUDE.md` → TypeScript strict flags.

## Learnings

_(Add agent-runtime-specific discoveries here.)_

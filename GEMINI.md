# SERA — Gemini Context

> For Gemini CLI and Google Antigravity. Shared rules are in `AGENTS.md`. This file covers Gemini-specific context.

## Quick start

1. Read `AGENTS.md` first — it has the universal rules
2. Read `docs/ARCHITECTURE.md` for design decisions
3. Read the workspace-level instruction file for your target area (e.g., `core/CLAUDE.md`, `web/CLAUDE.md`)

## Environment

- **Platform:** Windows 11 / bash shell — use Unix paths (forward slashes)
- **Working directory:** `D:/projects/homelab/sera`
- **Package manager:** npm workspaces — `core/` and `web/` are workspace packages
- **Dev start:** `npm run dev:up` (Docker Compose)

## Key commands

```bash
# Validation loop — run all three before opening a PR
npm run typecheck
npm run lint
npm run test

# Workspace-specific
npm run typecheck:core
npm run typecheck:web
npm run test --workspace=core
npm run test --workspace=web
```

## Architecture pointers

| Area | Key file(s) |
|---|---|
| API routes | `core/src/routes/*.ts` |
| Agent lifecycle | `core/src/agents/Orchestrator.ts`, `core/agent-runtime/` |
| LLM routing | `core/src/llm/LlmRouter.ts`, `core/src/llm/ProviderRegistry.ts` |
| Web dashboard | `web/src/pages/*.tsx`, `web/src/components/*.tsx` |
| DB migrations | `core/migrations/` |
| Epic specs | `docs/epics/*.md` |

## Gotchas

- `cd` does not persist between shell calls — always use absolute paths
- Shell scripts mounted into Docker must use LF line endings
- `npx` doesn't work in Git Bash — use direct node paths (e.g., `node web/node_modules/typescript/bin/tsc`)
- Dev Docker uses named volumes for `node_modules` — run `docker compose ... down -v` to force fresh install

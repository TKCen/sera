# Development

Resources for contributing to SERA.

## Getting Set Up

```bash
git clone https://github.com/TKCen/sera.git
cd sera
npm install          # Use npm on Windows (not bun — see CLAUDE.md)
bun run dev:up       # Start the dev stack with hot-reload
```

## Code Quality

```bash
bun run ci           # Full CI pipeline (format check + lint + typecheck + test + build)
bun run check-all    # Full local check (format + lint + typecheck + test + build)
bun run pre-commit   # Quick pre-commit check (typecheck + lint + web tests)
```

## Project Structure

| Workspace             | Language                | Build                 | Test         |
| --------------------- | ----------------------- | --------------------- | ------------ |
| `core/`               | TypeScript (Node.js 22) | `tsup`                | `vitest`     |
| `web/`                | TypeScript (React 19)   | `vite build`          | `vitest`     |
| `tui/`                | Go                      | `go build`            | `go test`    |
| `cli/`                | Go                      | `go build`            | `go test`    |
| `e2e/`                | TypeScript              | —                     | `playwright` |
| `core/agent-runtime/` | TypeScript (Bun)        | — (runs .ts directly) | `vitest`     |

## Key References

- [Testing Strategy](../TESTING.md) — test categories, infrastructure, conventions
- [Agent Workflow](../AGENT-WORKFLOW.md) — multi-agent development coordination
- [Skill Ecosystem](../SKILL-ECOSYSTEM.md) — skill library design
- [Migrations](../MIGRATIONS.md) — database migration patterns

## Workspace Instruction Files

Each workspace has its own `CLAUDE.md` with environment-specific details:

| File                           | Covers                                           |
| ------------------------------ | ------------------------------------------------ |
| `CLAUDE.md` (root)             | Cross-cutting environment concerns               |
| `core/CLAUDE.md`               | API server, DB patterns, TypeScript strict flags |
| `web/CLAUDE.md`                | React patterns, API client, Centrifugo hooks     |
| `core/agent-runtime/CLAUDE.md` | Bun runtime, reasoning loop, container image     |
| `cli/CLAUDE.md`                | Go CLI, auth flow                                |
| `e2e/CLAUDE.md`                | Playwright patterns                              |

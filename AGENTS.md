# SERA — Agent Instructions

> Cross-agent instruction file. Read by Jules, Gemini CLI, Antigravity, Codex, and any AGENTS.md-compatible tool.
> For Claude Code specifics, see `CLAUDE.md`. For Gemini-specific config, see `GEMINI.md`.

## Project overview

SERA (Sandboxed Extensible Reasoning Agent) is a Docker-native multi-agent AI orchestration platform.
- **Monorepo:** `core/` (Node/TypeScript API), `web/` (React/Vite dashboard), `tui/` (Go TUI), `e2e/` (Playwright)
- **Package manager:** bun workspaces
- **Runtime:** Docker Compose (Postgres, Centrifugo, sera-core, sera-web)

## Before you start

1. Read `docs/ARCHITECTURE.md` — canonical source of truth for all design decisions
2. Check the GitHub issue you're working on for acceptance criteria and linked epic
3. Load only the docs relevant to your task (see table in `CLAUDE.md`)

## Code standards

- TypeScript strict mode, no `any` types
- No inline/dynamic imports — use top-level `import` statements only
- No backwards-compatibility hacks for removed code
- Follow existing patterns in the file you're editing
- Keep changes minimal — don't refactor surrounding code unless the issue asks for it

## Testing requirements

- **Unit tests** for pure logic (no I/O, < 5ms each)
- **Integration tests** for DB/service interactions
- **All new code must have tests** — see `docs/TESTING.md` for strategy
- Run validation before marking work complete: `bun run typecheck && bun run lint && bun test`

## Validation loop (mandatory before PR)

Every agent must run this sequence and confirm all steps pass:

```bash
bun run typecheck        # Zero errors
bun run lint             # Zero warnings (web: --max-warnings 0)
bun test                 # All tests pass
```

If any step fails, fix the issue and re-run. Do not open a PR with failing checks.

## Branch and commit conventions

- **Branch naming:** `<agent>/<issue-number>-<short-description>`
  - Examples: `jules/42-add-budget-api`, `claude/55-fix-memory-compaction`, `gemini/68-provider-registry`
  - Agent prefixes: `claude/`, `jules/`, `gemini/`, `antigravity/`, `human/`
- **Commit messages:** imperative mood, reference issue number
  - Example: `feat(core): add budget tracking endpoint (#42)`
- **One logical change per PR** — don't bundle unrelated work

## PR conventions

- Title: `<type>(scope): description (#issue)`
- Body must include: Summary (what + why), Test plan, Issue reference
- Assign the `agent-work` label
- Open as **draft** by default — human promotes to ready-for-merge
- Request review from `@TKCen` (human reviewer)

## File structure awareness

Each workspace has its own `CLAUDE.md` / instruction context:
- `core/CLAUDE.md` — API server specifics, DB patterns, migration rules
- `web/CLAUDE.md` — React patterns, component conventions, API client
- `e2e/CLAUDE.md` — E2E test patterns
- `core/agent-runtime/CLAUDE.md` — Agent container runtime

Read the relevant workspace instruction file before making changes in that workspace.

## Executable workflows

The `.agents/workflows/` directory contains step-by-step runnable workflows:

**Delegation:**
- `delegate-to-jules.md` — Create a `jules`-labeled issue for async auto-pickup
- `delegate-to-gemini.md` — Run Gemini CLI locally with validation + retry loop
- `work-orchestrator.md` — Decompose a goal → delegate to Jules + Gemini → integrate

**Integration & validation:**
- `integrate-agent-pr.md` — The critical loop: rebase → validate → fix (via Gemini, max 3 retries) → merge or escalate
- `validate.md` — Run typecheck + lint + test, delegate fixes to Gemini if failing

**Coordination:**
- `create-agent-issue.md` — Create a properly labeled issue for agent pickup
- `review-agent-prs.md` — Batch review all open agent PRs

See `docs/AGENT-WORKFLOW.md` for the full coordination protocol.

## What NOT to do

- Don't modify `docker-compose.yaml` or `docker-compose.dev.yaml` without explicit approval
- Don't add new dependencies without checking if an existing one covers the need
- Don't create new files when editing an existing one would work
- Don't touch files outside the scope of your assigned issue
- Don't push directly to `main` — always use a feature branch + PR
- **Don't commit log files** (`*.log`, `backend*.log`) — they may contain internal paths and config
- **Don't commit test-only artifacts** without the actual implementation they claim to support
- **Don't apply patterns from other frameworks** (e.g. Next.js `app/` directory) — follow existing project conventions
- **Don't create empty PRs** — verify your branch has actual changes before opening a PR

## PR quality checklist (self-check before opening)

Every PR must pass this checklist. If any item fails, fix it before opening:

- [ ] `git diff --stat` shows the expected files — no log files, no unrelated changes
- [ ] Changes match the PR title/description — no extra files snuck in
- [ ] Code follows existing patterns in the file being edited (not patterns from other frameworks)
- [ ] No `any` types introduced, no `@ts-ignore` without a comment explaining why
- [ ] The implementation is complete — not just a test file or stub claiming to be a feature
- [ ] `bun run typecheck && bun run lint && bun test` all pass

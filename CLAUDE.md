# SERA — Project Instructions

## Working Principles

Behavioral guidelines that override tempting shortcuts. Apply to every change, including those delegated to sub-agents.

### 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it — don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: every changed line should trace directly to the request.

### 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

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
| `docs/plan/`                   | SERA 2.0 MVS specs — Rust migration plans and phase specs                         |
| `rust/CLAUDE.md`               | Rust workspace — crate map, toolchain, dev workflow                               |

## Environment

- **Platform:** WSL2 / bash shell — use Unix syntax (forward slashes) throughout
- **Working directory:** `/home/entity/projects/sera`
- **Package manager:** Rust (via Cargo) — active workspace in `rust/`
- **`cd` does not persist between shell calls** — use absolute paths in every command

## Codebase map

```
sera/
  rust/                  # Rust workspace (SERA 2.0, active)     → see rust/CLAUDE.md
  scripts/               # Dev/ops scripts (sera-local, sera-omc, sera-omx, …)
  docs/                  # Architecture and spec docs             (load on demand — see table above)
  e2e/                   # Playwright E2E tests                   → see e2e/CLAUDE.md
  examples/              # Example manifests and agents
  ops/                   # Ops tooling (docker, systemd, etc.)
  capability-policies/   # CapabilityPolicy definitions
  secrets/               # Local secrets (gitignored)
  workspaces/            # Agent workspaces
  legacy/                # Archived pre-Rust codebase (reference only; no active development)
    core/                # Former sera-core API server (TS)
    core/agent-runtime/  # Former agent worker (TS)
    web/                 # Former sera-web dashboard
    tui/                 # Former Rust TUI (pre-reboot)
    cli/                 # Former Go CLI (auth flows)
    tools/discord-bridge/ # Former Discord sidecar
    centrifugo/          # Former Centrifugo config
    templates/ schemas/ sandbox-boundaries/ lists/ circles/ …    # Other pre-migration assets
```

> **Note (2026-04-21):** The TS/Go codebase was moved under `legacy/` during the Rust migration. Active work lives in `rust/`; `legacy/` is kept for reference while Rust parity is finished. Any sections that reference `core/`, `web/`, `tui/`, `cli/`, or `tools/` at top-level paths (outside `legacy/`) reflect the pre-migration layout — apply with `legacy/` prefix if touching them.

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

## Learnings

> Most learnings live in the workspace-specific CLAUDE.md files (`core/CLAUDE.md`, `web/CLAUDE.md`, etc.). Only cross-cutting items belong here.

- **Git Bash mangles absolute Linux paths in `docker exec`**: Prefix with `MSYS_NO_PATHCONV=1` when passing paths like `/app/...` to `docker exec`.
- **Squid egress proxy fails on `docker restart`**: The squid PID file persists across restarts, causing `FATAL: Squid is already running`. Workaround: `docker compose down sera-egress-proxy && docker compose up -d sera-egress-proxy`. See #363.
- **API endpoints require auth header**: All `/api/*` endpoints (except `/api/health/*`) require `Authorization: Bearer <key>`. Dev key: `sera_bootstrap_dev_123`.
- **Prettier format check differs between Windows and Linux**: Jules PRs formatted on Linux may fail CI format check when our pre-commit hook reformats on Windows. Always run `bun run format` before pushing.
- **Use `sera-omc`/`sera-omx` for monitored sessions**: Run `scripts/sera-omc [bead-id]` instead of bare `omc` to launch a clawhip-monitored tmux session with Discord notifications (keywords: "✻ Worked for", "● APPROVED", "✓ Closed", "FATAL", etc.). Use `scripts/sera-omx [bead-id]` for Codex (OMX) sessions. Both scripts auto-claim the bead if provided and name the session `omc-sera-<bead-id>`.


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->

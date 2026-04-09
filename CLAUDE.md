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
| `docs/plan/`                   | SERA 2.0 MVS specs — Rust migration plans and phase specs                         |
| `rust/CLAUDE.md`               | Rust workspace — crate map, toolchain, dev workflow                               |

## Environment

- **Platform:** Windows 11 / bash shell — use Unix syntax (forward slashes) throughout
- **Working directory:** `D:/projects/homelab/sera`
- **Package manager:** bun workspaces (`core/` and `web/` are workspace packages)
- **`cd` does not persist between shell calls** — use absolute paths in every command

## Codebase map

```
sera/
  core/                  # sera-core API server (TS)    → see core/CLAUDE.md
  core/agent-runtime/    # Agent worker process (TS)    → see core/agent-runtime/CLAUDE.md
  rust/                  # Rust workspace (SERA 2.0)    → see rust/CLAUDE.md
  web/                   # sera-web dashboard           → see web/CLAUDE.md
  tui/                   # Rust terminal UI (ratatui)   → see tui/CLAUDE.md
  cli/                   # Go CLI (auth flows)          → see cli/CLAUDE.md
  tools/discord-bridge/  # Discord sidecar              → see tools/discord-bridge/CLAUDE.md
  e2e/                   # Playwright E2E tests         → see e2e/CLAUDE.md
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

> Most learnings live in the workspace-specific CLAUDE.md files (`core/CLAUDE.md`, `web/CLAUDE.md`, etc.). Only cross-cutting items belong here.

- **Use `npm install` on Windows, `bun install` in Docker**: Bun 1.3.x on Windows creates junction points that Windows treats as "untrusted mount points" — Node.js, tsc, and other tools cannot traverse them. Use `npm install` on the Windows host for compatible `node_modules`. Docker builds use `bun install` via `bun.lock`. Both `package-lock.json` and `bun.lock` must be kept in sync. Use `bunx` to run local binaries (e.g. `bunx vitest run`, `bunx tsc --noEmit`).
- **Git Bash mangles absolute Linux paths in `docker exec`**: Prefix with `MSYS_NO_PATHCONV=1` when passing paths like `/app/...` to `docker exec`.
- **Squid egress proxy fails on `docker restart`**: The squid PID file persists across restarts, causing `FATAL: Squid is already running`. Workaround: `docker compose down sera-egress-proxy && docker compose up -d sera-egress-proxy`. See #363.
- **API endpoints require auth header**: All `/api/*` endpoints (except `/api/health/*`) require `Authorization: Bearer <key>`. Dev key: `sera_bootstrap_dev_123`.
- **Prettier format check differs between Windows and Linux**: Jules PRs formatted on Linux may fail CI format check when our pre-commit hook reformats on Windows. Always run `bun run format` before pushing.


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

# SERA Dogfeed Protocol v1

## Overview

One cycle = one task from the tracker → code → verify → merge → learn.

**Trigger:** `POST /api/dogfeed/run` or `sera_dogfeed_run` MCP tool, or human asks OMC to "run a dogfeed cycle."

## Two-Tier Agent Routing

| Task complexity                        | Agent                               | Cost                | When                 |
| -------------------------------------- | ----------------------------------- | ------------------- | -------------------- |
| Trivial (lint, TODO, dead imports)     | pi-agent + Qwen 3.5 35B (LM Studio) | **$0.00**           | Can run continuously |
| Complex (tests, refactors, multi-file) | OMC (Claude Opus, subscription)     | Subscription tokens | Manual trigger       |

Routing is automatic based on task category in `DOGFEED-TASKS.md`.

## Prerequisites

- SERA core running (or host dev environment)
- On `main` branch with clean working tree
- CI baseline green: `bun run typecheck && bun run lint && bun test`
- For pi-agent: LM Studio running with Qwen 3.5 35B loaded at `localhost:1234/v1`
- For OMC: Docker available, `sera-dogfeed-agent:latest` image built

## Cycle Steps

### 1. Analyze — Pick Task

Read `docs/DOGFEED-TASKS.md`, parse ready tasks, select the highest-priority unblocked task.

If no tasks available, report "No unblocked tasks" and end.

### 2. Route — Select Agent

Based on task category:

- `lint`, `todo`, `dead-code` → **pi-agent** (trivial, free)
- Everything else → **OMC** (complex, subscription)

### 3. Branch

```bash
git checkout -b dogfeed/<short-id>-<slug> main
```

### 4. Execute

**pi-agent path:**

```bash
pi --model "qwen/qwen3.5-35b-a3b" --provider lmstudio --print --no-session "<task prompt>"
```

**OMC path:**
Spawn `sera-dogfeed-agent` Docker container with:

- Bind-mounted repo at `/workspace`
- Host `~/.claude/` mounted at `/root/.claude/` (read-only)
- Task passed via `DOGFEED_TASK` env var

### 5. Verify

```bash
bun run typecheck
bun run lint
bun test
```

All three must pass (exit code 0). On failure:

- If caused by agent changes: attempt fix or abort
- If pre-existing: record as environment issue, abort cycle

### 6. Commit & Push

```bash
git add <specific files only>
git commit -m "dogfeed(<scope>): <description>

Co-Authored-By: SERA Dogfeed <noreply@sera.dev>"
git push -u origin dogfeed/<branch-name>
```

### 7. Merge

```bash
git checkout main
git merge dogfeed/<branch-name> --no-ff
git push origin main
git branch -d dogfeed/<branch-name>
git push origin --delete dogfeed/<branch-name>
```

### 8. Record Learning

Update `docs/DOGFEED-TASKS.md` — move task to Done section with:

- Outcome (OK/FAILED)
- Token estimate
- Duration
- Files changed

Append row to `docs/DOGFEED-PHASE0-LOG.md`.

## On Failure

If the cycle fails at any step after branching:

1. Record failure reason
2. Checkout main, delete the branch
3. Update task tracker with failure info

If the same task fails 2 times, deprioritize it.

## Auth (Phase 0)

- **pi-agent:** Uses LM Studio on localhost — no auth needed
- **OMC host mode:** Uses existing `~/.claude/` credentials
- **OMC Docker mode:** Host `~/.claude/` bind-mounted into container (read-only)
- **GitHub:** Uses `gh` CLI auth from host environment

## Token Tracking

- Estimate tokens per cycle
- Record in task tracker close reason
- Append to `docs/DOGFEED-PHASE0-LOG.md`
- Running total tracked against 500k Phase 0 budget

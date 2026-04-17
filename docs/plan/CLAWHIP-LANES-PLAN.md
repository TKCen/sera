# CLAWHIP + OMC/OMX Dev Orchestration for SERA
**Date:** 2026-04-16  
**Goal:** Implement Froschi-style lane orchestration for SERA development using clawhip + OMC + OMX

---

## Context

The SERA codebase already has:
- `clawhip` v0.6.6 installed at `~/.cargo/bin/clawhip`
- `sera-omc` and `sera-omx` wrapper scripts in `scripts/`
- `ops/lanes.md` with lane routing policy (OMC = default, OMX = rate-limited deep-diagnosis, Gemini = large-context intake)
- `scripts/lib/beads.sh` integration with `bd` (beads CLI)
- `ops/tmux/` directory (empty shell)
- Discord channel `1493263362753302749` wired into scripts

What Froschi described (and what we need) is a **full lane orchestration system** with:

| Missing piece | What Froschi showed |
|---|---|
| Canonical task artifact contract | `task.json` + `TASK-NNN.md` per task |
| Planner lane | Writes bounded task files; uses `omc --madmax --plan` |
| Executor lane | Uses `omx exec / ralph / team` per task metadata |
| Review lane | `omx exec` with dedicated review-role prompt |
| Handoff artifacts | `artifacts/handoffs/TASK-NNN-HANDOFF.md` |
| Review artifacts | `artifacts/review/TASK-NNN-REVIEW.md` |
| Operator decisions file | `artifacts/handoffs/OPERATOR-DECISIONS.md` |
| Signal surface (5 verbs) | HANDOFF / BLOCKED / REVIEW / DONE / FAILED |
| Per-issue worktree model | `~/source/<project>-wt/<slug>/` |
| Wrapper CLI commands | `plan <project> <slug>`, `exec <project> <slug>`, `review <project> <slug>`, `worktree mark-reclaimable` |
| Lane pause/resume | Live tmux session stays alive on BLOCKED; answers injected via `tmux send-keys` |
| Fast-path routing | Skip OMC planning when task is already anchored (file:line, issue#, exact spec) |
| Multi-issue parallel lanes | Per-slug worktrees on same repo, each on own branch |
| Worktree bootstrap hook | Per-project shell script to set up deps after `git worktree add` |
| Failure recovery | Wrapper ledger + tmux query + artifact presence as recovery truth |

---

## What's Already Working (Smoke Test)

```
scripts/sera-omc          ← launches omc in clawhip-monitored tmux session
scripts/sera-omx          ← launches omx in clawhip-monitored tmux session  
scripts/lib/beads.sh      ← bd integration for bead handoffs
clawhip tmux new           ← creates monitored tmux sessions
clawhip send              ← sends custom events to Discord
```

---

## What Needs to Be Built

### Phase 1 — Core Wrapper CLI & Task Artifact Contract

**1.1** Create `scripts/sera` wrapper entry point
```
sera plan <slug> --issue N --message "..."    # creates worktree + planner lane
sera exec <slug>                               # dispatch per task.json
sera review <slug>                             # review lane
sera worktree mark-reclaimable <slug> --pr N  # mark + gc
sera status <slug>                             # lane state from ledger
```

**1.2** Define canonical task artifact schema `artifacts/tasks/TASK-NNN.json`
```json
{
  "taskId": "TASK-001",
  "title": "Add MemoryBlock to sera-types",
  "titleSlug": "memory-block-types",
  "lane": "omc",
  "intent": "implement",
  "cwd": "/home/entity/projects/sera",
  "goal": "Add MemoryBlock struct to sera-types with priority, recency_boost, char_budget fields",
  "inputs": ["rust/crates/sera-types/src/lib.rs"],
  "writeArtifacts": ["artifacts/handoffs/TASK-001-HANDOFF.md"],
  "stopCondition": "MemoryBlock struct compiles and has unit tests",
  "dependencies": [],
  "verify": ["cargo build -p sera-types", "cargo test -p sera-types"],
  "reviewRequired": true,
  "execution": {
    "mode": "exec",
    "teamSize": 1
  }
}
```

Required fields: `taskId`, `title`, `cwd`, `goal`, `inputs`, `writeArtifacts`, `stopCondition`, `dependencies`, `verify`, `reviewRequired`, `execution.mode`

Optional / wrapper-overridable: `titleSlug`, `lane`, `intent`, `role`, `execution.teamSize`

**1.3** Create `scripts/lib/task.sh` — task artifact utilities
- `task_create <taskId> <goal> <cwd>` → writes `artifacts/tasks/<taskId>.json`
- `task_read <taskId>` → parse and print task fields
- `task_handoff_write <taskId> <result>` → write `artifacts/handoffs/<taskId>-HANDOFF.md`
- `task_review_write <taskId> <verdict> <findings>` → write `artifacts/review/<taskId>-REVIEW.md`
- `task_verify <taskId>` → run verify commands from task

**1.4** Create `scripts/lib/wrapperLedger.sh`
JSON ledger at `~/.sera/wrapper ledger.json`:
```json
{
  "slug": "issue-142-memory-block",
  "branch": "issue-142-memory-block",
  "cwd": "/home/entity/projects/sera-worktrees/issue-142-memory-block",
  "status": "active",
  "created_at": "2026-04-16T18:00:00Z",
  "last_lane": "omc",
  "session_names": ["omc-issue-142-memory-block-r1"],
  "pr": null,
  "issue": 142,
  "taskId": "TASK-001",
  "artifact_root": ".omc/runtime/issue-142-memory-block/"
}
```

Operations: `ledger_create`, `ledger_update`, `ledger_get`, `ledger_list_active`, `ledger_mark_reclaimable`

---

### Phase 2 — Worktree Lifecycle

**2.1** Create `scripts/lib/worktree.sh`
- `worktree_create <slug> <issue>` — `git worktree add ~/source/sera-wt/<slug> -b <slug> origin/sera20`
- `worktree_remove <slug>` — `git worktree remove` after reclaimable
- `worktree_list` — list all sera worktrees
- `worktree_bootstrap <slug>` — run per-project bootstrap hook

**2.2** Create per-project bootstrap hook `ops/worktree-bootstrap.sh`
```bash
#!/usr/bin/env bash
set -euo pipefail
WT="$1"
# Symlink .env if exists
ln -sfn ../.env .env 2>/dev/null || true
# Install deps
cargo fetch 2>/dev/null || true
```

**2.2** GitHub issue → lane dispatch
```bash
# On github.issue-opened event:
sera plan issue-142 --issue 142 --message "Add MemoryBlock to sera-types"
```

**Base branch:** `origin/sera20` (confirmed to exist — `e2ed26660edf83e651d02b7d284c7f2d9edde403`)

---

### Phase 3 — Lane Implementations

**3.1** Planner lane (`sera plan`)
```bash
# Creates worktree + task.json + launches OMC planner
clawhip tmux new \
  --session "omc-${SLUG}-r${RUN}" \
  --channel "${DISCORD_CHANNEL}" \
  --keywords "${KEYWORDS}" \
  --cwd "${WORKTREE_CWD}" \
  --attach \
  -- omc --madmax --plan "Read artifacts/tasks/${TASK_ID}.md. Confirm bounded, execution-ready. Write plan to artifacts/handoffs/${TASK_ID}-HANDOFF.md. Stop."
```

**3.2** Executor lane (`sera exec`)
```bash
# Reads task.json, launches appropriate OMX mode
clawhip tmux new \
  --session "omx-${SLUG}-r${RUN}" \
  --channel "${DISCORD_CHANNEL}" \
  --keywords "${KEYWORDS}" \
  --cwd "${WORKTREE_CWD}" \
  --attach \
  -- omx exec --dangerously-bypass-approvals-and-sandbox --high \
     "Read artifacts/tasks/${TASK_ID}.json. Implement. Write artifacts/handoffs/${TASK_ID}-HANDOFF.md. Stop."
```

Mode selection:
- `mode=exec` → `omx exec`
- `mode=ralph` → `omx ralph` (verify/fix loop)
- `mode=team` → `omx team` (parallel, multiple agents)

**3.3** Review lane (`sera review`)
```bash
clawhip tmux new \
  --session "review-${SLUG}-r${RUN}" \
  --channel "${DISCORD_CHANNEL}" \
  --keywords "${KEYWORDS}" \
  --cwd "${WORKTREE_CWD}" \
  --attach \
  -- omx exec --dangerously-bypass-approvals-and-sandbox --high \
     "Review artifacts/handoffs/${TASK_ID}-HANDOFF.md and changed files. Verify task goal met. Write artifacts/review/${TASK_ID}-REVIEW.md with verdict + findings."
```

---

### Phase 4 — Signal Surface & Discord Integration

**4.1** Clawhip signal envelope (JSON over Discord)
```json
{
  "verb": "HANDOFF",
  "taskId": "TASK-001",
  "session": "omx-issue-142-memory-block-r1",
  "cwd": "/home/entity/source/sera-wt/issue-142-memory-block",
  "artifactPath": "artifacts/handoffs/TASK-001-HANDOFF.md",
  "timestamp": "2026-04-16T18:30:00Z"
}
```

Signal verbs:
| Verb | Meaning |
|---|---|
| `HANDOFF` | Lane complete, artifact ready for next lane |
| `BLOCKED` | Lane waiting on operator input (lane stays alive) |
| `REVIEW` | Review lane complete, verdict issued |
| `DONE` | Task fully complete including review |
| `FAILED` | Lane failed; wrapper waiting for operator decision |

**4.2** Clawhip route config `~/.clawhip/config.toml`
```toml
[providers.discord]
token = "${SERA_DISCORD_CLAWHIP_BOT_TOKEN}"
default_channel = "1493263362753302749"

[[routes]]
event = "session.blocked"
sink = "discord"
channel = "1493263362753302749"

[[routes]]
event = "session.finished"
sink = "discord"
channel = "1493263362753302749"

[[routes]]
event = "session.failed"
sink = "discord"
channel = "1493263362753302749"

[[routes]]
event = "tmux.stale"
sink = "discord"
channel = "1493263362753302749"
```

---

### Phase 5 — Fast-Path & Operator Decisions

**5.1** Fast-path routing
Skip planner when request has strong anchors:
- Exact file:line reference
- Exact issue number + confirmed bounded goal
- Exact function/struct name
- Exact spec section reference

Detection: regex match in incoming message before invoking planner.

**5.2** Operator decisions file
```bash
# When planner emits open questions:
clawhip send --event "agent.blocked" \
  --meta question="Which tier policy should MemoryBlock use?" \
  --channel "${DISCORD_CHANNEL}"

# Operator answers via:
sera decide <slug> --question-id 1 --answer "tier-2 for now, promote if needed"

# Answer written to artifacts/handoffs/OPERATOR-DECISIONS.md
# Executor reads it; if conflicts with task.json → stop and escalate
```

---

### Phase 6 — Lane Pause/Resume (BLOCKED pattern)

**6.1** Lane stays alive on BLOCKED
When executor hits a clarifying question:
1. Write `BLOCKED` signal to Discord with question
2. Do NOT exit tmux session — leave it alive
3. Wrapper records `status: waiting_on_operator` in ledger

**6.2** Answer injection
```bash
# Operator types answer via CLI:
sera answer <slug> --text "Use tier-2 policy, promote to tier-1 only if char_budget > 8000"

# Wrapper injects into live tmux:
tmux send-keys -t "omx-${SLUG}-r${RUN}" "Use tier-2 policy, promote to tier-1 only if char_budget > 8000" Enter
```

**6.3** Timeout policy
- Paused lanes: no auto-fail timeout (operator decides)
- Janitor job: after 24h pause, surface `STALE` warning in Discord

---

### Phase 7 — Failure Recovery

**7.1** On SIGNAL:FAILED
```bash
# Diagnose:
clawhip tmux tail -s "omx-${SLUG}-r${RUN}"
git -C "${WORKTREE_CWD}" status
ls "${WORKTREE_CWD}/.omc/runtime/${SLUG}/"

# Decision tree:
# artifact exists + failure late  → resume same task in same worktree
# no artifact + died early       → relaunch in same worktree
# wrong scope/cwd/bad assumptions → stop, re-plan
```

**7.2** Crash recovery on restart
```bash
# Wrapper reads ledger on startup:
for entry in ledger_list_active; do
  # Query tmux for session
  if tmux has-session -t "${entry.session_names[0]}" 2>/dev/null; then
    # Check artifact presence
    if [[ -f "${entry.artifact_root}/HANDOFF.md" ]]; then
      ledger_update status=review_ready
    else
      ledger_update status=running
    fi
  else
    # tmux dead — check artifact
    if [[ -f "${entry.artifact_root}/HANDOFF.md" ]]; then
      ledger_update status=completed
    else
      ledger_update status=stale
    fi
  fi
done
```

---

## Implementation Order

```
Week 1:
├── Phase 1 (wrapper CLI + task schema + ledger)
├── Phase 2 (worktree lifecycle)
└── Smoke test: plan → exec → review on one real issue

Week 2:
├── Phase 3 (all three lane types wired)
├── Phase 4 (signal surface + Discord routing)
└── End-to-end: GitHub issue → lane → Discord signal → PR

Week 3:
├── Phase 5 (fast-path + decisions file)
├── Phase 6 (lane pause/resume)
└── Phase 7 (failure recovery + crash restart)

Ongoing:
- Refine KEYWORDS for Discord routing
- Tune fast-path anchor detection regex
- Add per-project bootstrap hooks as repos are onboarded
```

---

## Key Files to Create/Modify

```
scripts/sera                      ← main wrapper CLI entry point
scripts/lib/task.sh               ← task artifact CRUD
scripts/lib/wrapperLedger.sh      ← JSON ledger operations
scripts/lib/worktree.sh           ← worktree lifecycle
scripts/lib/lane.sh               ← launch planner/exec/review lanes
scripts/lib/signal.sh             ← send Discord signal envelopes
ops/worktree-bootstrap.sh          ← per-project dep setup
~/.clawhip/config.toml             ← Discord routing + routes
artifacts/tasks/                  ← canonical task artifacts
artifacts/handoffs/               ← HANDOFF.md + OPERATOR-DECISIONS.md
artifacts/review/                 ← REVIEW.md per task
```

---

## Verification

1. **Unit test:** `sera plan issue-N --message "..."` creates correct `task.json` + worktree + launches OMC
2. **Unit test:** `sera exec <slug>` reads task.json and launches correct OMX mode
3. **E2E:** Real GitHub issue → Discord HANDOFF → exec → REVIEW → DONE signal
4. **Pause test:** Lane hits BLOCKED → tmux session stays alive → `sera answer` injects → lane resumes
5. **Crash test:** Wrapper process killed during lane → restarted → correctly classifies running vs stale

---

## Open Questions

1. **Discord bot token:** Is `SERA_DISCORD_CLAWHIP_BOT_TOKEN` already configured, or do we need a new dedicated bot per clawhip recommendation?
2. **Base branch:** `origin/sera20` for all new worktrees (confirmed exists)
3. **Fast-path anchor detection:** Should this be regex-based in the wrapper, or trust the operator to use a `/fast` flag?
4. **Lane pause implementation:** Froschi used `tmux send-keys` — should we use a named pipe instead for reliability?
5. **Existing clawhip config:** ✅ `~/.clawhip/config.toml` exists, fully configured with Discord routes

---

## References

- Froschi/gaebal-gajae conversation (this plan is derived from)
- `clawhip` v0.6.7 docs: https://github.com/Yeachan-Heo/clawhip
- `scripts/sera-omc`, `scripts/sera-omx` (existing scripts)
- `scripts/lib/beads.sh` (existing beads integration)
- `ops/lanes.md` (existing lane routing policy)

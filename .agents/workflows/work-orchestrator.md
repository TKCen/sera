---
description: Break down a goal and delegate to Jules + Gemini agents in parallel
---

This workflow decomposes a large goal into subtasks, delegates them to the
appropriate agents, then integrates the results. It is the top-level coordination
workflow — it calls into `delegate-to-jules`, `delegate-to-gemini`, and
`integrate-agent-pr` as sub-workflows.

---

### 1. Sync with remote

// turbo
```bash
git fetch origin main
git checkout main
git pull origin main
```

### 2. Break down the goal

Decompose the work into independent, non-overlapping subtasks. For each subtask decide:

| Size | Overlap risk | Agent | Workflow |
|---|---|---|---|
| Large feature, multi-file | Low (isolated area) | Jules | `delegate-to-jules` |
| Small fix, single file | None | Gemini CLI | `delegate-to-gemini` |
| Complex, cross-cutting | Any | Claude Code | Handle directly |

**Rules for decomposition:**
- Each subtask must touch **different files** — no two agents editing the same file
- Each subtask must be **independently testable**
- List affected files in every issue body to prevent collisions
- Sequence dependent tasks with `blocked-by:#N` in the issue body

### 3. Delegate Jules tasks (async, parallel)

For each Jules-sized subtask, create an issue. Jules auto-picks up on the `jules` label.

// turbo
```bash
gh issue create \
  --title "<type>(<area>): <subtask description>" \
  --label "agent-ready,jules,assign-to-jules" \
  --body "<detailed description with acceptance criteria and file list>"
```

### 4. Delegate Gemini tasks (local, sequential or parallel)

For each Gemini-sized subtask, create a branch and run Gemini.
See `delegate-to-gemini` workflow for the full pattern.

// turbo
```bash
git checkout -b gemini/<issue>-<name> origin/main
gemini --sandbox -y -p "<subtask prompt with full context>"
bash scripts/validate.sh
```

If validation passes, push:
// turbo
```bash
git add -A
git commit -m "<type>(<scope>): <description> (#<issue>)"
git push -u origin HEAD
gh pr create --draft --title "<type>(<scope>): <description>" --body "Closes #<issue>"
```

### 5. Monitor Jules progress

// turbo
```bash
gh pr list --state open --label jules
```

### 6. Integrate each PR when ready

For each completed PR (Gemini or Jules), run the integration workflow.
See `integrate-agent-pr` for the full validate → fix → merge loop.

// turbo
```bash
gh pr list --state open --label agent-work --json number,title,headRefName
```

**Integration order matters:** merge PRs that others depend on first.

### 7. Post-integration validation

After all PRs are merged, run a final validation on main:

// turbo
```bash
git checkout main
git pull origin main
bash scripts/validate.sh
```

If anything fails, identify which merge broke it and revert:
// turbo
```bash
gh pr revert <breaking-pr> --title "revert: PR #<N> broke main"
```

# Multi-Agent Development Workflow

This document defines how multiple AI coding agents coordinate work on SERA. It covers agent roles, work assignment, validation loops, and handoff patterns.

---

## Agent roster

| Agent | Strengths | Best for | Coordination file |
|---|---|---|---|
| **Claude Code** (desktop) | Deep context, multi-file refactors, architecture | Complex features, cross-cutting changes, planning | `CLAUDE.md` |
| **Jules** (jules.google.com) | Async background work, GitHub-native | Bug fixes, isolated features, test writing | `AGENTS.md` |
| **Gemini CLI** | Fast iteration, terminal-native | Quick fixes, scripting, single-file changes | `GEMINI.md` |
| **Antigravity** | IDE-integrated, visual feedback | UI work, component building, CSS/layout | `AGENTS.md` + `GEMINI.md` |
| **Human** | Judgment, approval, architecture decisions | Code review, issue triage, merge decisions | — |

---

## Work assignment flow

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐     ┌───────────┐
│ Create issue │────▶│ Label + assign│────▶│ Agent works  │────▶│ Open PR   │
│ (human/agent)│     │ to agent     │     │ on branch    │     │ for review│
└─────────────┘     └──────────────┘     └──────────────┘     └───────────┘
                                                                     │
                                              ┌──────────────┐       │
                                              │ Human reviews │◀──────┘
                                              │ + merges      │
                                              └──────────────┘
```

### 1. Issue creation

All work starts as a GitHub issue. Issues must have:
- **Title:** clear, actionable description
- **Body:** acceptance criteria, relevant epic/story reference, affected files/areas
- **Labels:** `agent-ready` + complexity label + area label (see Labels section)

### 2. Agent assignment

Assign issues based on agent strengths:

| Complexity | Scope | Assign to |
|---|---|---|
| Simple bug fix, single file | `core/` or `web/` | Jules or Gemini CLI |
| UI component or page | `web/` | Antigravity |
| Multi-file feature | Any | Claude Code |
| Architecture change | Cross-cutting | Claude Code (plan first) |
| Test coverage gaps | Any | Jules |
| Quick script or config | Root / scripts | Gemini CLI |

Use the `assign-to-<agent>` label to signal which agent should pick up the work:
- `assign-to-claude`
- `assign-to-jules` (also add the `jules` label — Jules auto-picks up issues with this label and creates a PR)
- `assign-to-gemini`
- `assign-to-antigravity`

> **Jules auto-pickup:** Adding the `jules` label to any issue triggers Jules to automatically start working on it and open a PR when done. No manual intervention needed.

### 3. Agent picks up work

Each agent, when starting work:
1. Reads the issue description and acceptance criteria
2. Reads `AGENTS.md` (universal rules)
3. Reads the relevant workspace instruction file (`core/CLAUDE.md`, `web/CLAUDE.md`, etc.)
4. Reads the linked epic doc if referenced (`docs/epics/*.md`)
5. Creates a branch: `<agent>/<issue-number>-<short-description>`
6. Implements the change
7. Runs the validation loop
8. Opens a PR referencing the issue

### 4. Validation loop (every agent, every PR)

```bash
# Step 1: Type safety
npm run typecheck
# Must exit 0 with zero errors

# Step 2: Lint
npm run lint
# Must exit 0 with zero warnings

# Step 3: Tests
npm run test
# Must exit 0, all tests pass

# Step 4: (if touching e2e-relevant code) E2E smoke
# npm run test --workspace=e2e
```

**Agents must not open a PR until all steps pass.** If a step fails, fix and re-run.

---

## Issue labels

### Status labels
| Label | Meaning |
|---|---|
| `agent-ready` | Issue is fully specified and ready for an agent |
| `agent-work` | PR was created by an agent |
| `needs-human` | Agent is blocked, needs human input |
| `in-review` | PR is open and awaiting review |

### Assignment labels
| Label | Meaning |
|---|---|
| `assign-to-claude` | Best suited for Claude Code |
| `assign-to-jules` | Best suited for Jules |
| `assign-to-gemini` | Best suited for Gemini CLI |
| `assign-to-antigravity` | Best suited for Antigravity |

### Complexity labels
| Label | Meaning |
|---|---|
| `complexity:trivial` | < 30 min, single file, obvious fix |
| `complexity:small` | 1-2 files, well-defined scope |
| `complexity:medium` | 3-5 files, requires reading architecture docs |
| `complexity:large` | 5+ files, needs planning phase |

### Area labels
| Label | Meaning |
|---|---|
| `area:core` | `core/` workspace |
| `area:web` | `web/` workspace |
| `area:e2e` | `e2e/` tests |
| `area:infra` | Docker, CI, scripts |
| `area:docs` | Documentation only |

---

## Agent patterns

### Writer / Reviewer
For medium+ complexity tasks, use two agents with isolated contexts:
1. **Writer agent** implements the feature on a branch
2. **Reviewer agent** (different context window) reviews the diff for bugs, missed edge cases, style violations
3. This prevents confirmation bias — the reviewer starts fresh

### Plan / Execute
For large tasks, split planning from implementation:
1. **Planning phase** (expensive model like Opus): produce `implementation-strategy.md` with file list, approach, risks
2. **Execution phase** (faster model like Sonnet/Gemini): follow the strategy doc step by step
3. Strategy doc is committed to the branch so any agent can resume

### Research / Implement
Before touching code, the agent produces a research summary:
1. Read the issue, linked epic, and relevant architecture docs
2. Write a brief research summary as a PR comment or in-branch doc
3. Implement based on findings — this prevents hallucinated architecture

---

## Handoff patterns

### Agent → Human (PR review)
Every agent PR requires human review before merge. The PR must:
- Be opened as **draft** (human promotes to ready-for-merge)
- Include: summary (what + why), test plan, issue reference (`Closes #N`)
- Have the `agent-work` label (auto-applied by CI for agent branches)

### Agent → Agent (sequential work)
When one agent's work depends on another's:
1. First agent completes their PR and it gets merged
2. Second agent's issue is labeled `blocked-by:#N` until the dependency merges
3. Second agent pulls latest `main` before starting

### Human → Agent (feedback loop)
When a reviewer requests changes on an agent PR:
1. Add review comments on the PR
2. Re-assign to the same agent (or a different one)
3. Agent addresses feedback on the same branch, pushes, re-runs validation

### Escalation
If an agent is stuck or producing poor results:
1. Agent (or human) adds `needs-human` label to the issue
2. Human reviews and either:
   - Provides more context in the issue and re-assigns
   - Takes over the work manually (re-labels as `assign-to-human`)
   - Breaks the issue into smaller pieces

---

## Parallel work guidelines

Multiple agents can work simultaneously if their issues touch **non-overlapping files**. To prevent conflicts:

1. Each issue should list affected files/areas in the description
2. Don't assign two agents to the same file simultaneously
3. If overlap is unavoidable, sequence the work (use `blocked-by`)
4. Keep PRs small — smaller PRs merge faster and reduce conflict windows

---

## Context efficiency

Each agent should load **only what it needs**:

| What to read | When |
|---|---|
| `AGENTS.md` | Always (universal rules) |
| `CLAUDE.md` / `GEMINI.md` | Always (agent-specific config) |
| `docs/ARCHITECTURE.md` | Any implementation task |
| `docs/epics/<N>-*.md` | When working on that epic |
| `docs/TESTING.md` | When writing tests |
| `docs/openapi.yaml` | When adding/modifying API endpoints |
| `core/CLAUDE.md` | When touching `core/` |
| `web/CLAUDE.md` | When touching `web/` |

**Do not** load all docs upfront. Load incrementally as needed.

---

## Executable workflows

The `.agents/workflows/` directory contains step-by-step runnable workflows (Antigravity `// turbo` format):

| Workflow | Purpose |
|---|---|
| `work-orchestrator.md` | Top-level: decompose goal → delegate to Jules + Gemini → integrate results |
| `delegate-to-jules.md` | Create a `jules`-labeled issue for async auto-pickup and PR creation |
| `delegate-to-gemini.md` | Run Gemini CLI locally with validation + retry loop (max 3 attempts) |
| `integrate-agent-pr.md` | **The integration loop:** rebase → validate → fix (via Gemini) → merge or escalate |
| `validate.md` | Run typecheck + lint + test; delegate fixes to Gemini if failing |
| `create-agent-issue.md` | Create a well-labeled GitHub issue for agent pickup |
| `review-agent-prs.md` | Batch review all open agent PRs |
| `development.md` | Standard development cycle (plan → build → verify) |
| `deployment.md` | Deploy the full SERA stack |

These workflows are designed for Antigravity's `// turbo` format but the steps work for any agent.

---

## Quality gates

### Before merge (human reviewer checks)
- [ ] CI passes (typecheck + lint + test)
- [ ] Changes match the issue's acceptance criteria
- [ ] No unrelated changes included
- [ ] Tests cover the new/changed behavior
- [ ] No new `any` types introduced
- [ ] No security issues (injections, credential leaks)

### Periodic (human-initiated)
- Review open agent PRs weekly
- Close stale issues/PRs older than 14 days
- Audit label accuracy monthly

### Jules recurring tasks
Jules runs scheduled maintenance tasks that produce PRs automatically.
See `docs/jules-recurring-tasks.md` for the full prompt library:
- **Weekly:** type safety sweep, dead code cleanup, test coverage gaps
- **Bi-weekly:** dependency audit, TODO/FIXME sweep
- **Monthly:** API docs sync, console.log cleanup

Review these PRs via `integrate-agent-pr` workflow — most are quick-review.

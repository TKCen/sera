---
description: Create a well-structured GitHub issue for agent assignment
---

This workflow creates a GitHub issue formatted for the multi-agent workflow, with proper labels for assignment and tracking.

1. Determine the appropriate labels based on the task:
   - **Agent assignment:** `assign-to-claude`, `assign-to-jules`, `assign-to-gemini`, `assign-to-antigravity`
   - **Jules auto-pickup:** Also add the `jules` label — Jules will automatically start working and open a PR
   - **Complexity:** `complexity:trivial`, `complexity:small`, `complexity:medium`, `complexity:large`
   - **Area:** `area:core`, `area:web`, `area:e2e`, `area:infra`, `area:docs`

2. Create the issue with structured body and labels.
// turbo
```bash
gh issue create \
  --title "<type>(<area>): <description>" \
  --label "agent-ready,assign-to-<agent>,complexity:<level>,area:<area>" \
  --body "## Task
<detailed description>

## Acceptance criteria
- [ ] <criterion 1>
- [ ] <criterion 2>
- [ ] Typecheck, lint, and tests pass

## Affected files
- <file or directory>

## Context
- Epic: <epic reference if applicable>
- Docs: <relevant doc links>"
```

3. Verify the issue was created.
// turbo
```bash
gh issue list --state open --label "agent-ready" --limit 5
```

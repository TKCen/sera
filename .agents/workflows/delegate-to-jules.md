---
description: Delegate a task to Jules via a GitHub issue (auto-pickup with jules label)
---

Jules auto-picks up issues labeled `jules` and works asynchronously in a cloud VM.
It opens a PR when done. Use this for features, bug fixes, and test coverage tasks
that are well-scoped and self-contained.

### 1. Create a detailed issue with the `jules` label

The issue body is the only context Jules gets. Be explicit: list files to touch,
acceptance criteria, and link to relevant docs.

// turbo
```bash
gh issue create \
  --title "<type>(<area>): <description>" \
  --label "agent-ready,jules,assign-to-jules" \
  --body "## Task
<what needs to happen — be specific>

## Acceptance criteria
- [ ] <criterion>
- [ ] Typecheck, lint, and tests pass

## Files likely affected
- <path>

## Context
Read \`AGENTS.md\` for project rules.
Read \`docs/epics/<N>-*.md\` for epic context.
Read \`core/CLAUDE.md\` or \`web/CLAUDE.md\` for workspace rules.

Run validation before opening the PR:
\`\`\`bash
npm run typecheck && npm run lint && npm run test
\`\`\`"
```

### 2. Wait for Jules to complete (async — can take up to an hour)

// turbo
```bash
gh issue list --state open --label jules --json number,title,state
```

### 3. When Jules opens a PR, move to the `integrate-agent-pr` workflow

// turbo
```bash
gh pr list --state open --label jules --json number,title,headRefName
```

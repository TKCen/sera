---
description: Orchestrate work across jules and gemini agents using git worktrees
---

This workflow orchestrates complex tasks by delegating subtasks to jules.google for larger PRs and the local gemini CLI for smaller tasks, utilizing git worktrees to keep the main development environment isolated and clean during integration.

1. Break down the overarching goal into chunks suitable for:
   - jules.google (larger architectural changes, full features)
   - gemini CLI (smaller scopes, local isolated tasks)

2. Create a new git worktree for managing and testing these orchestrations without polluting your current working environment.
// turbo
```bash
git branch orchestrator-integration
git worktree add ../sera-orchestrator orchestrator-integration
cd ../sera-orchestrator
```

3. Delegate the smaller, scoped tasks to the local `gemini` CLI, following the `outsource-cli` workflow.
// turbo
```bash
gemini run --prompt "<subtask description>"
```

4. Delegate the larger tasks to jules by creating GitHub issues, following the `outsource-jules` workflow.
// turbo
```bash
gh issue create --title "<task title>" --body "<detailed task description>" --label "jules"
```

5. Track progress and when jules completes the work and opens pull requests, check them out within the worktree to review as per the `integrate-pr` workflow.
// turbo
```bash
gh pr list --state open --label jules
gh pr checkout <pr-number>
```

6. Run local tests and verify the code within the isolated worktree environment.

7. If the code looks good, approve and merge the pull request into the main branch.
// turbo
```bash
gh pr review <pr-number> --approve
gh pr merge <pr-number> --merge
```

8. Clean up the worktree once all orchestrations and integrations are merged successfully.
// turbo
```bash
cd -
git worktree remove ../sera-orchestrator
git branch -D orchestrator-integration
```

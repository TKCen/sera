---
description: Orchestrate work across jules and gemini agents using git worktrees
---

This workflow orchestrates complex tasks by delegating subtasks to jules.google for larger PRs and the local gemini CLI for smaller tasks. Utilizing git worktrees, it enables parallel agentic code development by running multiple LLM-driven development sessions concurrently. Each session operates in its own isolated sandboxed environment to ensure reproducible experiments and enforce privacy.

1. Fetch all updates from the remote.
// turbo
```bash
git fetch --all
```

2. Break down the overarching goal into concurrent chunks suitable for:
   - jules.google (larger architectural changes, full features)
   - gemini CLI (smaller scopes, local isolated tasks)

3. Create a new git worktree for the orchestration environment.
// turbo
```bash
git worktree add -b <task-branch-name> .worktrees/<task-name> main
cd .worktrees/<task-name>
```

4. Delegate the smaller, scoped tasks to the local `gemini` CLI in sandbox mode.
// turbo
```bash
gemini --sandbox -y -p "<subtask description>"
```

5. Delegate the larger tasks to jules by creating GitHub issues.
// turbo
```bash
gh issue create --title "<task title>" --body "<detailed task description>" --label "jules"
```

6. Track progress and when jules completes the work and opens pull requests, prepare a worktree for each PR and then delegate the integration to the gemini CLI.
// turbo
```bash
git worktree add .worktrees/#prx<pr-number> <pr-branch-name>
cd .worktrees/#prx<pr-number>
gemini --sandbox -y -p "Integrate the changes from PR #<pr-number> into main. Resolve any merge conflicts with main, run local tests."
```

7. Clean up the task-specific worktree once the overall orchestration is complete.
// turbo
```bash
cd -
git worktree remove .worktrees/<task-name> --force
```

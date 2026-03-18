---
description: Offload smaller tasks to the gemini CLI in delegation mode
---

This workflow delegates smaller, scoped tasks to the local `gemini` CLI for parallel agentic code development. By utilizing git worktrees, you can run multiple LLM-driven development sessions concurrently in isolated sandboxed environments, avoiding branch collisions and ensuring reproducible experiments.

1. Fetch all updates from the remote.
// turbo
```bash
git fetch --all
```

2. Break down the current task into discrete, small subtasks that can be processed in parallel by multiple `gemini` CLI sessions.

3. Create an isolated git worktree for the subtask to maintain independent file states.
// turbo
```bash
git worktree add -b <subtask-branch> .worktrees/<subtask-name> main
cd .worktrees/<subtask-name>
```

4. Run the subtask via the `gemini` CLI in sandbox mode.
// turbo
```bash
gemini --sandbox -y -p "<subtask description>"
```

5. If specific extensions or workflows are required for the subtask, include them when triggering the CLI.
// turbo
```bash
gemini --sandbox -y -p "<subtask description>" --workflow "<workflow-name>" --extension "<extension-name>"
```

6. Track the output and verify the results locally.

7. Clean up the worktree once the subtask is successfully integrated.
// turbo
```bash
cd -
git worktree remove .worktrees/<subtask-name> --force
```

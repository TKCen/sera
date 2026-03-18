---
description: Offload smaller tasks to the gemini CLI in delegation mode
---

This workflow delegates smaller, scoped tasks to the local `gemini` CLI, utilizing its workflows and extensions. In this flow, the main agent acts as the coordinator, managing the execution of these smaller tasks.

1. Break down the current task into a discrete, small subtask that can be solved locally by the `gemini` CLI.
2. Formulate a prompt for the CLI that provides clear boundaries and constraints.
3. Run the subtask via the `gemini` CLI.
// turbo
```bash
gemini run --prompt "<subtask description>"
```

4. If specific extensions or workflows are required for the subtask, include them when triggering the CLI.
// turbo
```bash
gemini run --prompt "<subtask description>" --workflow "<workflow-name>" --extension "<extension-name>"
```

5. Track the output and verify the results locally once the CLI completes the task.
6. Integrate the changes into the broader goal and continue coordinating further subtasks as needed.

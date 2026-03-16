---
description: Offload a task to jules.google via GitHub Issues
---

1. Create a new GitHub issue with a detailed task description, technical expectations, and context.
```bash
gh issue create --title "[TASK NAME]" --body "Detailed description and expectations..."
```

2. Assign the `jules` label to the issue to trigger the automated work process.
```bash
gh issue edit [ISSUE_NUMBER] --add-label "jules"
```

3. Monitor Jules' progress using the Jules CLI.
```bash
jules status
```

4. Review the generated Pull Request. 
   - If the changes meet expectations, merge them.
   - If changes are insufficient, provide feedback in the PR or discard and refine the issue.

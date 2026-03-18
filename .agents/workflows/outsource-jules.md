---
description: Create tasks for jules.google via GitHub issues
---

This workflow creates detailed task descriptions for jules.google and kicks off automated work by creating GitHub issues with the `jules` label.

1. Formulate a detailed task description with technical constraints and context.
2. Create a new GitHub issue with the `jules` label. This label will automatically kick off the work by jules.google.
// turbo
```bash
gh issue create --title "<task title>" --body "<detailed task description>" --label "jules"
```

3. Track the progress using the jules CLI (note: it may take up to an hour until work is completed).
// turbo
```bash
jules status
```

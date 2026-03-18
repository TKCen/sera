---
description: Integrate changes proposed by jules into the main branch
---

This workflow reviews and integrates PRs proposed by jules.google.

1. List the open pull requests to identify the changes proposed by jules.
// turbo
```bash
gh pr list --state open --label jules
```

2. Checkout the pull request locally to review the code and run verification tests.
// turbo
```bash
gh pr checkout <pr-number>
```

3. Run local tests and verify the code meets the project's standards.

4. If the code looks good, approve and merge the pull request into the main branch.
// turbo
```bash
gh pr review <pr-number> --approve
gh pr merge <pr-number> --merge
```

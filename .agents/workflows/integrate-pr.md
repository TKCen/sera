---
description: Integrate changes proposed by jules into the main branch
---

This workflow reviews and integrates PRs proposed by jules.google.

1. Fetch all updates from the remote.
// turbo
```bash
git fetch --all
```

2. List the open pull requests to identify the changes proposed by jules.
// turbo
```bash
gh pr list --state open --label jules
```

3. Create a dedicated task worktree and checkout the pull request locally to review the code and run verification tests.
// turbo
```bash
git worktree add .worktrees/#prx<pr-number> <pr-branch-name>
cd .worktrees/#prx<pr-number>
```

4. Delegate the integration to the gemini CLI.
// turbo
```bash
gemini --sandbox -y -p "Integrate the changes from PR #<pr-number> into main. Resolve any merge conflicts with main and run local tests."
```

5. If the code looks good and tests pass, approve and merge the pull request into the main branch, then clean up the worktree.
// turbo
```bash
gh pr review <pr-number> --approve
gh pr merge <pr-number> --merge
cd -
git worktree remove .worktrees/#prx<pr-number> --force
```

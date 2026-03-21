---
description: Review all open agent PRs and take action
---

This workflow reviews open PRs created by agents, checking quality and deciding on merge/feedback/discard.

1. List all open agent PRs.
// turbo
```bash
gh pr list --state open --label "agent-work"
```

2. For each PR, review the diff and CI status.
// turbo
```bash
gh pr view <pr-number> --comments
gh pr diff <pr-number>
gh pr checks <pr-number>
```

3. Decision phase for each PR:

   **Merge** — code is correct, CI passes, meets acceptance criteria:
   // turbo
   ```bash
   gh pr review <pr-number> --approve
   gh pr merge <pr-number> --merge
   ```

   **Request changes** — minor issues found:
   // turbo
   ```bash
   gh pr review <pr-number> --request-changes --body "Feedback: ..."
   ```

   **Discard** — fundamentally wrong approach:
   // turbo
   ```bash
   gh pr close <pr-number> --delete-branch
   ```

4. Check for stale agent issues (open > 14 days with no activity).
// turbo
```bash
gh issue list --state open --label "agent-ready" --json number,title,updatedAt --jq '.[] | select(.updatedAt < (now - 1209600 | todate))'
```

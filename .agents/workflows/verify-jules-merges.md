---
description: Verify and integrate or discard changes from jules.google
---

1. Check for open Pull Requests created by Jules.
```bash
gh pr list --author "@me" # Jules PRs often appear as authored by the local user or a specific bot
```

2. Inspect the changes in a specific PR.
```bash
gh pr diff [PR_NUMBER]
```

3. Run the project locally in development mode to verify functionality.
```bash
docker compose up -d
```

4. Decision Phase:
   - **Merge**: If the code is high quality and meets requirements.
     ```bash
     gh pr merge [PR_NUMBER] --merge
     ```
   - **Request Changes**: If minor fixes are needed, comment on the PR.
     ```bash
     gh pr comment [PR_NUMBER] --body "Feedback..."
     ```
   - **Discard**: If the approach is fundamentally wrong.
     ```bash
     gh pr close [PR_NUMBER] --delete-branch
     ```

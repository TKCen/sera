---
description: Validate, fix, and integrate an agent-created PR into main
---

This is the critical end-of-pipeline workflow. An agent (Jules, Gemini, etc.) has
opened a PR. Now we need to: validate it passes all checks, fix issues if it doesn't,
and either merge or discard.

This workflow can be run by a human, by Claude Code, or delegated to Gemini CLI.

---

### 1. Identify the PR to integrate

// turbo
```bash
gh pr list --state open --label agent-work --json number,title,headRefName,author
```

Pick one PR. Set variables for the rest of the workflow:
- `PR=<number>`
- `BRANCH=<headRefName>`

### 2. Fetch and checkout the PR branch

// turbo
```bash
git fetch origin
git checkout <BRANCH>
git pull origin <BRANCH>
```

### 3. Rebase onto latest main (catch conflicts early)

// turbo
```bash
git fetch origin main
git rebase origin/main
```

If conflicts occur, resolve them (or delegate to gemini):
```bash
gemini --sandbox -y -p "Resolve the merge conflicts in this branch. Keep the intent of the PR changes while incorporating the latest main. Then run: bun run typecheck && bun run lint && bun run test"
```

After resolving:
// turbo
```bash
git rebase --continue
```

### 4. Run full validation

// turbo
```bash
bash scripts/validate.sh
```

### 5. Handle validation results

**If all checks pass → go to step 7 (merge).**

**If checks fail → fix loop (step 6).**

### 6. Fix loop (max 3 iterations)

Delegate the fix to Gemini CLI with the exact error output:

// turbo
```bash
gemini --sandbox -y -p "This PR branch has validation failures. Fix them.

Errors:
<paste the typecheck/lint/test errors from step 4>

Rules:
- Only fix the errors — don't change anything else
- Read AGENTS.md for project conventions
- Run the full validation when done: bun run typecheck && bun run lint && bun run test"
```

After Gemini finishes, re-run validation:

// turbo
```bash
bash scripts/validate.sh
```

**If it passes now → commit the fixes and go to step 7.**

// turbo
```bash
git add -A
git commit -m "fix: address validation failures in agent PR #<PR>"
```

**If it still fails after 3 iterations → escalate.**

// turbo
```bash
gh pr comment <PR> --body "Automated validation failed after 3 fix attempts. Needs human review.

Remaining errors:
\`\`\`
<paste errors>
\`\`\`"
gh issue edit <linked-issue> --add-label "needs-human"
```

### 7. Push, approve, and merge

// turbo
```bash
git push origin <BRANCH>
gh pr review <PR> --approve --body "Validation passed (typecheck + lint + test)."
gh pr merge <PR> --merge --delete-branch
```

### 8. Verify merge and clean up

// turbo
```bash
git checkout main
git pull origin main
bash scripts/validate.sh
```

If post-merge validation fails on main, revert immediately:
```bash
gh pr revert <PR> --title "revert: PR #<PR> broke main"
```

### 9. Close the linked issue if acceptance criteria are met

// turbo
```bash
gh issue close <linked-issue> --reason completed
```

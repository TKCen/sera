---
description: Run the full validation loop (typecheck + lint + test)
---

Run this before opening or merging any PR. Works from any directory inside the repo,
including worktrees.

### Full validation (both workspaces)

// turbo
```bash
bash scripts/validate.sh
```

### Single workspace (faster for scoped changes)

// turbo
```bash
bash scripts/validate.sh core
```

// turbo
```bash
bash scripts/validate.sh web
```

### What it checks

1. **Typecheck** — `npm run typecheck` (zero errors)
2. **Lint** — `npm run lint` (zero warnings; web uses `--max-warnings 0`)
3. **Test** — `npm run test` (all pass)

All three steps run even if one fails — you see every failure at once.

### If it fails

Fix the issues and re-run. If delegating fixes to Gemini CLI:

// turbo
```bash
gemini --sandbox -y -p "Fix these validation errors, then re-run validation:

<paste errors>

Run when done: npm run typecheck && npm run lint && npm run test"
```

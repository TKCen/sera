---
description: Delegate a small scoped task to the local Gemini CLI
---

Use this to offload a small, well-defined task to Gemini CLI. The task runs locally
in sandbox mode. Good for single-file fixes, script writing, config changes.

### 1. Create a branch for the task

// turbo
```bash
git fetch origin main
git checkout -b gemini/<issue-number>-<short-name> origin/main
```

### 2. Run Gemini CLI with a detailed prompt

Include all context the agent needs inline — it won't read AGENTS.md automatically.
Reference specific files and acceptance criteria.

// turbo
```bash
gemini --sandbox -y -p "Task: <description>

Rules:
- Read AGENTS.md for project conventions
- TypeScript strict, no \`any\` types, top-level imports only
- Follow existing patterns in the files you edit

Files to modify:
- <path>

Acceptance criteria:
- <criterion>

When done, run: bun run typecheck && bun run lint && bun run test
Fix any failures before finishing."
```

### 3. Validate the result

// turbo
```bash
bash scripts/validate.sh
```

### 4. If validation passes, commit and push

// turbo
```bash
git add -A
git commit -m "<type>(<scope>): <description> (#<issue>)"
git push -u origin HEAD
```

### 5. If validation fails, send Gemini back to fix

// turbo
```bash
gemini --sandbox -y -p "The validation failed. Here are the errors:

<paste errors>

Fix these issues. Then re-run: bun run typecheck && bun run lint && bun run test"
```

Repeat steps 3-5 until validation passes (max 3 retries, then escalate to human).

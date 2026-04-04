# Writing Skills

Skills are Markdown documents that shape how agents think and behave. They're injected into the system prompt — not executed as code.

## Skill Document Format

```markdown
---
id: my-custom-skill
name: My Custom Skill
version: 1.0.0
category: engineering/python
tags: [python, quality, testing]
applies-to: [shell-exec, file-write]
requires: [security-best-practices]
---

# My Custom Skill

## Guidelines

Write your guidance here. This text will be injected into the agent's
system prompt when this skill is referenced in the agent's manifest.

## Examples

Include concrete examples of good and bad patterns.

### Good

- Use type hints on all function signatures
- Write docstrings for public functions

### Bad

- Using `eval()` or `exec()` on untrusted input
- Catching bare `Exception` without re-raising
```

## Frontmatter Fields

| Field        | Required | Description                              |
| ------------ | -------- | ---------------------------------------- |
| `id`         | Yes      | Unique kebab-case identifier             |
| `name`       | Yes      | Human-readable name                      |
| `version`    | Yes      | Semver version string                    |
| `category`   | Yes      | Slash-separated category path            |
| `tags`       | No       | Searchable tag list                      |
| `applies-to` | No       | Tool IDs this skill is relevant for      |
| `requires`   | No       | Other skill IDs that must also be loaded |

## Where to Place Skills

| Location           | Loaded                 | Purpose                         |
| ------------------ | ---------------------- | ------------------------------- |
| `skills/builtin/`  | Always                 | Core skills that ship with SERA |
| `skills/examples/` | On reference           | Example skills for learning     |
| Custom path        | Via SkillSource config | Community or operator skills    |

Skills hot-reload — update the file and the next agent run picks up the changes.

## Referencing Skills in Agents

In a template or agent manifest:

```yaml
skills:
  - my-custom-skill # by ID from frontmatter
  - typescript-best-practices
  - git-workflow
```

## Best Practices

1. **Be specific** — vague guidance doesn't help LLMs. Include concrete examples.
2. **Keep it focused** — one skill, one topic. Don't combine Python style with Git workflow.
3. **Include anti-patterns** — showing what NOT to do is as valuable as showing what to do.
4. **Version carefully** — agents pin skills by ID. Breaking changes should use a new ID.
5. **Use `requires`** — if your skill depends on security guidelines, declare it.
6. **Test with real agents** — run an agent with your skill and verify it follows the guidance.

# Skills vs MCP Tools

This is a critical distinction in SERA's design. Skills and tools serve complementary purposes and should not be conflated.

## At a Glance

| Aspect               | Skills                | MCP Tools                       |
| -------------------- | --------------------- | ------------------------------- |
| **What they are**    | Markdown documents    | Executable functions            |
| **When they run**    | Injected at startup   | Called during reasoning         |
| **Side effects**     | None — guidance only  | Yes — file I/O, API calls, etc. |
| **Where they live**  | `skills/` directory   | Agent container or MCP server   |
| **How they're used** | Shape agent behaviour | Perform actions                 |

## Skills — Guidance Documents

Skills are **versioned Markdown documents** injected into the agent's system prompt to shape behaviour _before_ the reasoning loop begins.

```markdown
---
id: typescript-best-practices
name: TypeScript Best Practices
version: 1.0.0
category: engineering/typescript
tags: [typescript, quality, patterns]
---

# TypeScript Best Practices

## Type Safety

- Avoid `any`. Use `unknown` and narrow with type guards.
- Prefer `interface` for public API shapes.
- Enable `strict: true` in tsconfig.

## Error Handling

- Use typed error classes extending `Error`.
- Wrap external I/O in explicit try/catch.
```

### How Skills Are Loaded

1. Agent manifest declares skills by ID: `skills: [typescript-best-practices, git-workflow]`
2. `SkillLibrary` reads documents from `skills/builtin/` and `skills/examples/`
3. `SkillInjector` assembles referenced skills into a `<skills>` block
4. Block is injected into the agent's system prompt at startup

Skills hot-reload — update a skill document and the next agent run picks it up.

### Built-in Skills

| Skill                       | Purpose                              |
| --------------------------- | ------------------------------------ |
| `typescript-best-practices` | TypeScript coding standards          |
| `git-workflow`              | Git branching and commit conventions |
| `security-best-practices`   | Security patterns and anti-patterns  |

## MCP Tools — Executable Functions

MCP tools are **callable implementations** that agents invoke during reasoning steps. They run code, produce side effects, and return structured results.

### Built-in Tools

| Tool              | Capability Gate            | Purpose                             |
| ----------------- | -------------------------- | ----------------------------------- |
| `file-read`       | `filesystem.read`          | Read file contents                  |
| `file-write`      | `filesystem.write`         | Write file contents                 |
| `file-list`       | `filesystem.read`          | List directory contents             |
| `shell-exec`      | `exec.shell`               | Execute shell commands              |
| `knowledge-store` | `memory.write`             | Store knowledge in memory           |
| `knowledge-query` | `memory.read`              | Query knowledge via semantic search |
| `web-search`      | `network.outbound`         | Search the web                      |
| `web-fetch`       | `network.outbound`         | Fetch a web page                    |
| `schedule-task`   | `seraManagement.schedules` | Create scheduled tasks              |
| `delegate-task`   | `seraManagement.agents`    | Delegate work to another agent      |

### MCP Server Protocol

External tools are provided by MCP servers — containerised executables that agents discover and call at runtime. SERA extends the base MCP protocol with:

- **Credential injection** via `X-Sera-Credential-*` headers
- **Acting context propagation** for delegation chains
- **Standard error codes** (`credential_unavailable`, `scope_exceeded`, etc.)

### sera-core as MCP Server

sera-core exposes its own MCP server for agents with `seraManagement` capabilities. This is how Sera orchestrates the instance — managing agents, circles, schedules, and channels as tool calls.

## The Design Philosophy

> Skills guide. Tools act.

Skills shape _how_ an agent thinks about a problem. Tools give it the means to _do_ something about it. Keeping them separate means:

- Skills carry zero sandbox escape risk (they're just text)
- Tools can be capability-gated without affecting guidance
- The community can publish skill packs without code execution concerns
- Agent behaviour is reproducible — same skills, same guidance

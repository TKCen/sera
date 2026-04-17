# SERA Skill Format (AgentSkills-compatible markdown)

**Status:** Phase 2 (bead sera-6q21) — skills can now be authored as single
`.md` files with YAML frontmatter. Legacy two-file JSON + YAML packs still
load via `SkillLoader::with_legacy_fallback`.

This document specifies the frontmatter schema, body conventions, and the
migration path from the legacy format.

---

## 1. File layout

A skill is a single Markdown file named `<skill-name>.md`. The file starts
with YAML frontmatter fenced by `---` lines, followed by a freeform body.

```markdown
---
name: code-review
version: 1.0.0
description: Review code for correctness, style, and security issues
triggers:
  - review
  - audit
tools:
  - read_file
  - search_code
mcp_tools:
  stdio_servers:
    - name: github
      command: npx
      args: ["-y", "@modelcontextprotocol/server-github"]
      env:
        GITHUB_TOKEN: "${GITHUB_TOKEN}"
model: claude-opus-4
context_budget_tokens: 4096
---

You are a senior code reviewer. When activated, you systematically examine...
```

The body is injected verbatim into the agent's context window when the
skill is active. SERA does **not** render the markdown — the LLM sees the
raw bytes, so markdown syntax (headers, lists, code fences) works as a
prompt-structuring tool.

---

## 2. Frontmatter schema

All fields except `name` are optional. Unknown fields are rejected so typos
surface as a loader error rather than a silent no-op.

| Field | Type | Default | Purpose |
|---|---|---|---|
| `name` | `string` | **required** | Unique identifier, `[a-z0-9-]{1,64}` |
| `version` | `string` | — | Semver (`MAJOR.MINOR.PATCH[-pre][+build]`) |
| `description` | `string` | — | One-line human-readable summary |
| `triggers` | `[string]` | `[]` | Keywords that activate the skill |
| `tools` | `[string]` | `[]` | Tool names bound to this skill |
| `mcp_tools.stdio_servers` | `[StdioServer]` | `[]` | Per-skill MCP stdio subprocesses |
| `mcp_tools.sse_servers` | `[UrlServer]` | `[]` | Per-skill MCP SSE endpoints |
| `mcp_tools.streamable_http_servers` | `[UrlServer]` | `[]` | Per-skill MCP streamable HTTP endpoints |
| `model` | `string` | — | Model override (e.g. `claude-opus-4`) |
| `context_budget_tokens` | `u32` | — | Soft ceiling for the body token count |
| `source` | `string` | — | Free-form origin tag (`builtin`, `marketplace`, …) |

`StdioServer` shape:

```yaml
- name: github
  command: npx
  args: ["-y", "@modelcontextprotocol/server-github"]
  env:
    GITHUB_TOKEN: "${GITHUB_TOKEN}"
```

`UrlServer` shape:

```yaml
- name: search
  url: https://mcp.example.com/sse
  env: {}
```

Environment interpolation (`${NAME}`) is resolved by the runtime, not the
loader. The loader stores the raw string.

### Validation rules

- `name` must match `^[a-z0-9-]{1,64}$`.
- `version`, when present, must match `^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?$`.
- Unknown top-level keys → `SkillsError::Format`.
- The file MUST start with `---\n` (CRLF also accepted).
- The closing `---` must appear on its own line.

---

## 3. Body conventions

- The body is the full markdown text after the closing frontmatter fence.
- Preserve leading blank lines — the loader keeps bytes verbatim.
- Structure the body as a prompt. Use `## Checklist`, `## Output Format`
  or similar H2 sections for readability; the LLM benefits from the
  structure even though SERA does not render it.
- Keep bodies under `context_budget_tokens` when that field is set. The
  runtime may truncate otherwise.

---

## 4. Discovery and multi-path loading

`SkillLoader::new(paths: Vec<PathBuf>)` accepts a priority-ordered list of
directories. First path wins on name collision; the loser is logged at
DEBUG. Typical chain:

1. `~/.config/sera/skills/` — user-global
2. `./.sera/skills/` — workspace-local
3. `agents/<agent_name>/skills/` — SERA-specific

Use `SkillLoader::with_legacy_fallback(md_paths, legacy_paths)` during
migration to also resolve legacy two-file packs.

### Progressive disclosure (`_index.yaml`)

A `MarkdownSkillPack` supports an optional `_index.yaml` file that lets
`list()` answer metadata queries without touching any `*.md` bodies:

```yaml
name: core
description: Built-in skills
version: 1.0.0
skills:
  - name: code-review
    description: Review code for correctness, style, and security issues
    version: 1.0.0
```

Regenerate the index after any skill change:

```rust
pack.regenerate_index().await?;
```

---

## 5. Migration from legacy format

The legacy format stored two files per skill:

- `<pack>/<skill>.json` — the `SkillDefinition`
- `<pack>/<skill>.yaml` — the `SkillConfig`

Use `SkillLoader::with_legacy_fallback` to run both formats side by side.
A dedicated one-shot converter (reads legacy pair → writes `<skill>.md`)
is deferred to a follow-up bead; the manual mapping is:

| Legacy field                            | Markdown frontmatter field     |
|-----------------------------------------|--------------------------------|
| `<skill>.json::name`                    | `name`                         |
| `<skill>.json::description`             | `description`                  |
| `<skill>.json::version`                 | `version`                      |
| `<skill>.yaml::tools`                   | `tools`                        |
| `<skill>.yaml::context_injection`       | body text                      |
| `<skill>.yaml::trigger::Event(k)`       | `triggers: [k]`                |
| `<skill>.yaml::mode`                    | runtime default `OnDemand`     |

`SkillConfig::context_injection` is replaced by the markdown body —
`SkillDefinition::body` carries the text forward.

---

## 6. Programmatic API

```rust
use sera_skills::{MarkdownSkillPack, SkillLoader, SkillPack};

let loader = SkillLoader::new(vec![
    "/etc/sera/skills".into(),
    "./.sera/skills".into(),
]);

let pack: MarkdownSkillPack = loader.load("core").await?;

// Metadata-only listing (no body reads).
for entry in pack.list().await? {
    println!("{}@{:?}: {:?}", entry.name, entry.version, entry.description);
}

// Lazy body load for one skill.
let def = pack.get_skill("code-review").await?.unwrap();
println!("body bytes: {}", def.body.unwrap().len());
```

---

## 7. Migration note (for skill authors)

1. Pick `name`, `version`, `description`; put them in YAML frontmatter.
2. Move `context_injection` text into the markdown body verbatim.
3. Copy `tools` into frontmatter; add `triggers` for keyword activation.

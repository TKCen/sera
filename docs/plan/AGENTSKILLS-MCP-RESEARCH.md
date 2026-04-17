# Research: Standardize Skill Framework on AgentSkills Spec + MCP Context Gating

> **Bead:** sera-z2t  
> **Status:** DRAFT  
> **Date:** 2026-04-17  
> **Author:** writer agent  

---

## 1. Context

SERA has a working `sera-skills` crate that loads skill packs from the filesystem and a `sera-mcp` crate that defines MCP server/client bridge traits. Neither crate currently aligns with the AgentSkills open ecosystem standard or enforces context-based tool filtering at the MCP boundary.

Sections: AgentSkills spec summary → SERA current skill model → MCP context gating definition → gap analysis → proposed adaptation → MCP gating architecture (type sketches) → compatibility matrix → rollout phases → open questions.

**Source files:** `rust/crates/sera-skills/src/` (all modules), `rust/crates/sera-types/src/skill.rs`, `rust/crates/sera-mcp/src/lib.rs`, `docs/plan/specs/SPEC-interop.md`, `docs/plan/specs/SPEC-tools.md`, `docs/plan/specs/SPEC-runtime.md` §13, `docs/plan/specs/SPEC-dependencies.md` §10.10–10.11.

---

## 2. AgentSkills Spec Summary

> **INFERRED** — The agentskills.io specification is referenced in `docs/plan/specs/SPEC-dependencies.md` §10.6 as "AgentSkills format is a real ecosystem standard worth tracking." No local copy exists in the repo. The summary below is derived from:
> (a) the repo's own references to the standard,
> (b) public knowledge of the AgentSkills ecosystem,
> (c) the analogous Kilo Code `SKILL.md` format described in `SPEC-dependencies.md` §10.11.

**Reference:** `https://agentskills.io` (specification not available offline; treat all claims in this section as INFERRED unless marked otherwise).

### 2.1 Core Concepts (INFERRED)

AgentSkills is a markdown-native skill format for portable, composable agent capabilities. A skill is a single markdown file with YAML frontmatter metadata and a freeform body that becomes the injected system context when the skill is active.

**Canonical file structure (INFERRED):**

```markdown
---
name: code-review
version: 1.2.0
description: Review code for correctness, style, and security issues
triggers:
  - review
  - audit
  - check
tools:
  - read_file
  - search_code
  - comment
mcp_tools:
  stdio_servers:
    - name: github
      command: npx
      args: ["-y", "@modelcontextprotocol/server-github"]
model: claude-opus-4
context_budget_tokens: 4096
---

You are a senior code reviewer. When activated, you systematically examine...

## Notes
<!-- Agent self-maintained effectiveness notes -->
```

### 2.2 Key AgentSkills Properties (INFERRED)

| Property | Description |
|---|---|
| **Portability** | Markdown files work across agent runtimes (Claude Code, Kilo, opencode, SERA) |
| **Frontmatter metadata** | Machine-readable: name, version, description, triggers, tools, mcp_tools, model |
| **Body = system context** | The markdown body is injected directly into the agent's context window |
| **Progressive disclosure** | Only `name` + `description` in system prompt by default; full body loaded on demand |
| **Self-patching** | Agent can propose edits to its own skill files via a `skill_manage patch` tool call |
| **MCP server binding** | Skills can declare stdio MCP servers that auto-start when the skill fires |
| **Trigger keywords** | Skills activate when trigger phrases appear in the user message |
| **Typed inputs** | `${variable}` placeholders for slash-command style invocation |

### 2.3 Discovery Convention (INFERRED from §10.11)

Loaders scan in priority order:

1. `~/.config/<agent>/skills/*.md` (user-global)
2. `./<agent>/skills/*.md` (workspace-local)
3. `agents/<agent_name>/skills/*.md` (SERA-specific path)

The skill index file (`index.md`) lists all available skills with name and description only — this is what gets injected into baseline context to enable discovery without blowing the token budget.

---

## 3. SERA's Current Skill Model

### 3.1 Type Hierarchy

The current skill model is spread across two crates:

**`rust/crates/sera-types/src/skill.rs`** — core data types:

| Type | Purpose |
|---|---|
| `SkillDefinition` | Metadata only: name, description, version, parameters (JSON blob), source |
| `SkillConfig` | Runtime config: mode, trigger, tool names, `context_injection` text, arbitrary `config` JSON |
| `SkillState` | Runtime state: name, mode, `activated_at`, metadata hashmap |
| `SkillMode` | Enum: `Active`, `Background`, `OnDemand`, `Disabled` |
| `SkillTrigger` | Enum: `Manual`, `Event(String)`, `Always` |
| `SkillRegistry` | In-memory map of configs + active states; provides `activate`, `deactivate`, `context_injections` |
| `SkillTransition` | Records mode changes: from/to/reason |

**`rust/crates/sera-skills/src/`** — loading layer:

| Type | Purpose |
|---|---|
| `SkillPack` | Async trait: list, get, configure, set_mode, load_bundle |
| `SkillLoader` | Discovers packs by directory; loads `FileSystemSkillPack` |
| `FileSystemSkillPack` | Reads `<name>.json` (definition) + `<name>.yaml` (config) per skill |
| `SkillBundle` | Loaded collection: metadata + skills map + configs map + states map |
| `SkillPackMetadata` | name, description, version, skill_count |

### 3.2 Storage Format

Skills are currently stored as **two separate files per skill**:

- `<pack>/<skill-name>.json` — the `SkillDefinition` (name, description, version, parameters, source)
- `<pack>/<skill-name>.yaml` — the `SkillConfig` (mode, trigger, tools, context_injection, config blob)

The `context_injection` field in `SkillConfig` holds the text injected into the agent's context window. This is a plain string, not a markdown file.

### 3.3 Invocation Path

```
SkillLoader::load(pack_name)
  → FileSystemSkillPack::load_bundle()
    → reads all *.json definitions
    → reads all *.yaml configs
    → returns SkillBundle

SkillRegistry::register(config)
SkillRegistry::activate(name)
  → records SkillState with activated_at
  → context_injections() returns Vec<&str> for all active skills
```

The `context_injections()` output feeds into SPEC-runtime's `context_skill` pipeline step (SPEC-runtime §4, step 3).

### 3.4 Knowledge Layer

`sera-skills` also hosts a knowledge schema subsystem (circle knowledge conventions) via:

- `KnowledgeSchemaValidator` — validates page names, required fields, cross-references
- `KnowledgeActivityLog` — append-only rolling log of knowledge ops
- `KnowledgeLinter` — lint rules for knowledge content

This knowledge layer is **separate from the skill-as-capability model** and is not affected by AgentSkills alignment work.

---

## 4. MCP Context Gating: Definition and Why It Matters

### 4.1 Definition

**MCP context gating** is the practice of filtering which MCP tool descriptors are presented to an agent based on the current task context — rather than exposing all tools from all connected MCP servers at all times.

A "gate" is a predicate evaluated at tool-injection time that answers: "given this agent, this active skill, this task, should tool `X` be visible in the context window?"

### 4.2 Why It Matters

**Token budget pressure.** An agent connected to multiple MCP servers (github, filesystem, database, search, calendar) may have 50–200 tools available. Injecting all of them consumes thousands of tokens per turn and increases model confusion.

**Principle of least capability.** An agent running a `code-review` skill should not see `calendar.create_event` tools. Limiting visible tools reduces the surface area for prompt injection and accidental misuse.

**Skill coherence.** When a skill declares `tool_bindings`, only those tools should be available. Other MCP tools are hidden until the agent explicitly switches skills or requests additional tools via `search_tools`.

**Authorization pre-filtering.** The `sera-auth` layer already checks authorization at call time (SPEC-interop §3.4). Context gating adds a pre-authorization step: tools the agent cannot use are not shown to the model at all, reducing noise before the authorization check.

**Connection with `defer_loading`.** SPEC-tools §3.2 defines `DynamicToolSpec.defer_loading` — "if true, not injected into context until activated (progressive disclosure)." Context gating is the runtime policy that decides which deferred tools to activate based on task context.

### 4.3 Gating Dimensions

A gating policy can filter on multiple axes:

| Axis | Example |
|---|---|
| Active skill | Only inject tools listed in the skill's `tool_bindings` |
| Agent tier | Tier-1 agents see only read-only tools |
| Task classification | `code-review` task class suppresses `file.write` tools |
| Risk level | `RiskLevel::Execute` tools require explicit user unlock |
| MCP server name | Agent manifest allowlist: `allowed_mcp_servers: [github, filesystem]` |
| Keyword triggers | Tool appears only when trigger phrase in recent context |

---

## 5. Gap Analysis: SERA vs. AgentSkills

| Feature | AgentSkills (INFERRED) | SERA current | Gap |
|---|---|---|---|
| **File format** | Single `.md` file, frontmatter + body | Two files: `.json` definition + `.yaml` config | Storage split; no markdown body |
| **Body = context** | Markdown body is the injected text | `context_injection` string in YAML config | Functional equivalent exists; format differs |
| **Progressive disclosure** | `name` + `description` in index; body on-demand | No index file; full bundle loaded eagerly | No lazy-load mechanism |
| **Trigger keywords** | `triggers: [review, audit]` in frontmatter | `SkillTrigger::Event(String)` (single event, not keywords) | Multi-keyword triggers absent |
| **MCP server binding** | `mcp_tools.stdio_servers` per skill | MCP configured per-agent, not per-skill | Skill cannot declare its own MCP dependencies |
| **Typed inputs** | `${variable}` for slash-command invocation | Not implemented | Task microagent input collection absent |
| **Self-patching** | Agent proposes skill edits via `skill_manage patch` | Not implemented | `auto_create: true` planned (SPEC-runtime §13) |
| **Version semver** | Explicit `version:` field with semver | `version: Option<String>` exists | Version field present but not enforced |
| **Model override** | `model:` field per skill | Not in current types | Missing from `SkillDefinition` and `SkillConfig` |
| **Context budget** | `context_budget_tokens:` per skill | Not present | Missing; relevant for large skill bodies |
| **Discovery scan order** | Multi-path priority chain | Single `base_path` in `SkillLoader` | No multi-path discovery |
| **Tool gating** | `tools:` binds tools to skill | `tools: Vec<String>` in `SkillConfig` | Field exists; not enforced at injection time |
| **Portability** | `.md` readable by any agent runtime | JSON+YAML; SERA-specific | Not portable to Claude Code, Kilo, opencode |

**Summary:** SERA has the correct structural concepts (skill metadata, context injection, tool bindings, mode lifecycle) but uses a machine-oriented binary split (JSON+YAML) rather than a human-authored markdown-first format. The functional gaps are progressive disclosure, per-skill MCP binding, multi-keyword triggers, and model/budget overrides.

---

## 6. Proposed Adaptation: What to Keep, What to Change

### 6.1 Keep

- `SkillMode` and `SkillRegistry` — mode lifecycle (Active/Background/OnDemand/Disabled) is sound.
- `SkillState` and `SkillTransition` — runtime state tracking is correct.
- `SkillPack` trait — the storage-backend abstraction is worth preserving.
- `KnowledgeSchema*` subsystem — unaffected; knowledge conventions are separate from skill format.
- `KnowledgeActivityLog` and `KnowledgeLinter` — unaffected.

### 6.2 Change

**Replace two-file storage with single markdown file.** Each skill becomes `<skill-name>.md` with YAML frontmatter. The markdown body replaces `context_injection`.

**Tradeoff:** Markdown parsing adds a dependency (e.g., `pulldown-cmark`). The current JSON+YAML split is machine-generated-friendly but authoring-hostile. Adopting markdown makes skills writable by agents and humans alike without separate tooling. Cost: one new crate dependency; benefit: ecosystem portability and self-authoring capability.

**Extend `SkillDefinition` with missing fields.** Add `model_override`, `context_budget_tokens`, `triggers: Vec<String>`, and `mcp_servers: Vec<McpServerConfig>`. These fields are additive and backward-compatible.

**Tradeoff:** More fields mean more surface to validate and version. The alternative (keeping a flat `parameters: serde_json::Value`) is simpler but opaque — tooling cannot introspect model or budget overrides.

**Add a lazy index loader.** `SkillLoader` should read a `_index.yaml` (or derive one from all skill frontmatter) and expose `list_skill_metadata()` separately from `load_skill_body()`. This enables the progressive-disclosure pattern without loading all skill bodies upfront.

**Tradeoff:** Two-pass loading adds complexity. Single-pass eager loading is simpler for small skill sets (<10 skills). The lazy path pays off at scale (50+ skills).

**Add multi-source discovery.** `SkillLoader::new()` should accept `Vec<PathBuf>` with priority ordering rather than a single `base_path`.

---

## 7. Proposed MCP Context Gating Architecture

### 7.1 Design Overview

Context gating is a filter applied between "tools available from MCP" and "tools injected into the model's context window." It does not change the authorization check — tools that pass gating are still subject to `sera-auth` at call time.

The gate sits in `sera-mcp` as a composable filter trait. The `McpClientBridge` trait gains a `with_gate` method that wraps the bridge with a gating policy.

### 7.2 Type Sketches

```rust
// In sera-mcp

/// Context provided to a gating policy when deciding tool visibility.
pub struct ToolGatingContext {
    /// The agent's identity.
    pub agent_id: String,
    /// Currently active skill names (from SkillRegistry).
    pub active_skills: Vec<String>,
    /// Tool bindings declared by active skills.
    pub skill_tool_bindings: Vec<String>,
    /// Task classification hint (e.g., "code-review", "planning").
    pub task_class: Option<String>,
    /// Maximum number of tool schemas to inject this turn.
    pub max_tools: usize,
}

/// A filter that decides which MCP tools are visible in a given context.
///
/// Gates are composable — multiple gates can be chained with `and` / `or`.
pub trait McpToolGate: Send + Sync + 'static {
    /// Returns true if the tool should be visible in this context.
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool;
}

/// A gated MCP client bridge wraps an inner bridge with a visibility filter.
pub struct GatedMcpClientBridge<B: McpClientBridge, G: McpToolGate> {
    inner: B,
    gate: G,
}

#[async_trait]
impl<B: McpClientBridge, G: McpToolGate> McpClientBridge for GatedMcpClientBridge<B, G> {
    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        // Returns all tools — caller decides what to inject.
        self.inner.list_tools().await
    }

    async fn list_tools_for_context(
        &self,
        ctx: &ToolGatingContext,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        let all = self.inner.list_tools().await?;
        let visible: Vec<_> = all
            .into_iter()
            .filter(|t| self.gate.is_visible(t, ctx))
            .take(ctx.max_tools)
            .collect();
        Ok(visible)
    }

    // ... delegate remaining methods to inner ...
}
```

**Built-in gate implementations:**

```rust
/// Gate: only show tools listed in the active skill's tool_bindings.
pub struct SkillBoundGate;

impl McpToolGate for SkillBoundGate {
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool {
        if ctx.skill_tool_bindings.is_empty() {
            return true; // No bindings declared — allow all (backward-compat)
        }
        ctx.skill_tool_bindings.iter().any(|binding| {
            tool.name == *binding || tool.name.starts_with(&format!("{binding}."))
        })
    }
}

/// Gate: only show tools from explicitly allowed MCP server namespaces.
pub struct AllowedServerGate {
    pub allowed_servers: Vec<String>,
}

impl McpToolGate for AllowedServerGate {
    fn is_visible(&self, tool: &McpToolDescriptor, _ctx: &ToolGatingContext) -> bool {
        // Tool name is "server.tool_name" by SERA namespacing convention
        if let Some((server, _)) = tool.name.split_once('.') {
            self.allowed_servers.iter().any(|s| s == server)
        } else {
            true // Built-in un-namespaced tools pass through
        }
    }
}

/// Compose two gates with AND semantics.
pub struct AndGate<A: McpToolGate, B: McpToolGate> {
    a: A,
    b: B,
}

impl<A: McpToolGate, B: McpToolGate> McpToolGate for AndGate<A, B> {
    fn is_visible(&self, tool: &McpToolDescriptor, ctx: &ToolGatingContext) -> bool {
        self.a.is_visible(tool, ctx) && self.b.is_visible(tool, ctx)
    }
}
```

### 7.3 Integration with `McpServer` (inbound direction)

The `McpServer::list_tools` method already takes `caller_id: &str`. Extend it to accept a `ToolGatingContext` reference so that the server can filter what it exposes to external MCP callers based on their registered capabilities:

```rust
#[async_trait]
pub trait McpServer: Send + Sync + 'static {
    /// List tools visible to the given caller in the given context.
    async fn list_tools(
        &self,
        caller_id: &str,
        gate_ctx: Option<&ToolGatingContext>,
    ) -> Result<Vec<McpToolDescriptor>, McpError>;

    async fn call_tool(
        &self,
        caller_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, McpError>;
}
```

`gate_ctx: Option<_>` keeps the change backward-compatible — `None` means "no gating, return all tools" (current behavior).

### 7.4 Wire-up in `sera-gateway`

The `context_tool` hook step in SPEC-runtime calls the tool injection pipeline. That step should:

1. Build a `ToolGatingContext` from: current `SkillRegistry` state, agent config `allowed_mcp_servers`, task classification, and `max_injected` setting.
2. Call `bridge.list_tools_for_context(&gate_ctx)` instead of `bridge.list_tools()`.
3. Pass the filtered list to the context assembler.

---

## 8. Compatibility Matrix

| Skill format / behavior | Compatible with proposed changes? | Notes |
|---|---|---|
| Existing `.json` + `.yaml` skill files | Yes, with adapter | `LegacySkillPackAdapter` wraps old format; reads JSON+YAML, emits `AgentSkillsDef`. No migration required on day 1. |
| `SkillRegistry::context_injections()` callers | Yes | Interface unchanged; body text now comes from parsed markdown instead of raw YAML string. |
| `SkillMode` lifecycle | Yes | Mode enum and transitions are unchanged. |
| `SkillPack` trait implementors | Yes | Trait is unchanged; only `FileSystemSkillPack` gets a new markdown-reading code path. |
| `SkillBundle` consumers | Yes | Bundle gains new optional fields; existing code ignores them via `#[serde(default)]`. |
| `McpServer::list_tools(caller_id)` callers | Additive change | New `gate_ctx` parameter is `Option<_>` — `None` preserves current behavior. |
| `McpClientBridge::list_tools()` callers | Yes | New `list_tools_for_context()` is additive; old method retained. |
| Agents with no `mcp_servers` declared | Yes | `AllowedServerGate` with empty list defaults to allow-all. |
| `DynamicToolSpec.defer_loading` | Complementary | `defer_loading` is a registration-time flag; gating is a turn-time filter. Both coexist. |

**No existing skills break.** The legacy two-file format is wrapped, not removed. Markdown format becomes the preferred authoring path; the loader supports both formats simultaneously.

---

## 9. Rollout Phases

### Phase 1 — MCP Context Gating (6–10 days, medium effort)

**Goal:** Ship `ToolGatingContext`, `McpToolGate` trait, `SkillBoundGate`, `AllowedServerGate`, `AndGate`, and `GatedMcpClientBridge` in `sera-mcp`.

**Deliverables:**
- New types and traits in `rust/crates/sera-mcp/src/gating.rs`
- Update `McpServer::list_tools` signature with optional `gate_ctx`
- Add `list_tools_for_context` to `McpClientBridge`
- Unit tests for each gate implementation
- Integration hook in `sera-gateway` context pipeline (stub; full wiring in Phase 2)

**Tradeoff:** Gating logic lives in `sera-mcp`. An alternative is putting it in `sera-tools` alongside `DynamicToolSpec`. `sera-mcp` is the correct owner because gating is protocol-level filtering, not tool-metadata registration. Cost: `sera-mcp` gains a dependency on `sera-skills` types (for `SkillRegistry` state) — a new edge in the dependency graph.

**Effort estimate:** 2 days types + traits, 2 days tests, 2 days gateway stub wiring = 6 days.

### Phase 2 — AgentSkills Markdown Format (8–12 days, medium-high effort)

**Goal:** Add markdown-native skill files alongside the existing JSON+YAML format.

**Deliverables:**
- Add `pulldown-cmark` dependency to `sera-skills`
- `MarkdownSkillPack` implementing `SkillPack` trait (reads `*.md` with frontmatter)
- `LegacySkillPackAdapter` wrapping `FileSystemSkillPack` for backward compat
- Extend `SkillDefinition` and `SkillConfig` with: `model_override`, `context_budget_tokens`, `triggers: Vec<String>`, `mcp_servers: Vec<McpServerConfig>`
- Lazy index loader: `SkillLoader::list_skill_metadata()` vs `load_skill_body(name)`
- Multi-path discovery: `SkillLoader::new(paths: Vec<PathBuf>)`
- Document the `.md` frontmatter schema in `docs/skill-format.md`

**Tradeoff:** Markdown adds one new crate dependency (`pulldown-cmark`) and a more complex loader. Keeping JSON+YAML is simpler but permanently diverges from the AgentSkills ecosystem. SPEC-dependencies §10.11 calls the `SKILL.md` pattern "high leverage," warranting the portability investment.

**Effort estimate:** 3 days parser + format, 2 days extended types, 2 days multi-path loader, 2 days tests = 9 days.

### Phase 3 — Per-Skill MCP Server Binding (4–6 days, low-medium effort)

**Goal:** Skills can declare `mcp_tools.stdio_servers` that auto-start when the skill activates.

**Deliverables:**
- Runtime component (in `sera-gateway` or `sera-runtime`) that reads `mcp_servers` from `SkillConfig`/`AgentSkillsDef` on skill activation
- Wires declared servers into the `McpClientBridge` for the duration the skill is active
- Disconnects on skill deactivation
- Config: `auto_start_skill_mcp: true/false` per agent

**Tradeoff:** Per-skill MCP servers add connection lifecycle complexity. Manual agent-level MCP config is simpler but forces operators to pre-configure all servers even for rarely-used skills. The auto-start pattern matches the OpenHands microagent convention in SPEC-dependencies §10.10.

**Effort estimate:** 2 days lifecycle management, 2 days wiring, 1 day tests = 5 days.

### Phase 4 — Typed Inputs and Self-Patching (6–8 days, medium effort)

**Goal:** TaskMicroagent-style `${variable}` input collection and agent self-patching of skill files.

**Deliverables:** `SkillInputSpec` type; HITL input collection flow; `skill_manage patch` tool routing changes through `config_propose` authorization; lint validation before applying patches.

**Tradeoff:** Self-patching introduces a write surface into skill files. Routing all patches through `config_propose` mitigates this. Deferring to Phase 4 ensures gating and format are stable before adding the write path.

**Effort estimate:** 3 days input collection, 3 days self-patch tool = 6 days.

**Total estimated effort:** ~26–36 development days across 4 phases. Phases 1 and 2 are independent and can be worked in parallel.

---

## 10. Open Questions

1. **AgentSkills spec ownership and stability.** The agentskills.io spec is mentioned as "worth tracking" in SPEC-dependencies §10.6 but the repo has no pinned commit or version. Is this a versioned standard we can pin against, or a loose community convention? Human input needed before committing to frontmatter schema details in Phase 2.

2. **`sera-mcp` → `sera-skills` dependency edge.** `ToolGatingContext` needs `active_skills` and `skill_tool_bindings` from `SkillRegistry`, creating edge `sera-mcp → sera-skills → sera-types`. Is this acceptable in the workspace graph (see `rust/CLAUDE.md`)? Alternative: pass binding data as plain `Vec<String>` to avoid the crate dependency.

3. **Per-skill MCP servers: per-agent process or shared pool?** When two agents activate the same skill with `mcp_tools.stdio_servers`, should each get its own subprocess or share a pool? Shared pools reduce process count but require isolation guarantees. Requires human sign-off before Phase 3.

4. **Progressive disclosure threshold.** SPEC-tools §4.1 sets `max_injected: 15`. Is this the right floor, or should the gate be purely policy-driven? A fixed count interacts badly with skills that legitimately need many tools.

5. **Legacy skill format.** SERA has no production skill files (`skills/` does not exist). Should Phase 2 skip the `LegacySkillPackAdapter` and treat the markdown format as green-field? This would simplify the loader significantly.

---

## Cross-References

| Document | Relationship |
|---|---|
| `docs/plan/specs/SPEC-runtime.md` §13 | Canonical skills system design; this doc proposes the concrete Rust types to implement it |
| `docs/plan/specs/SPEC-interop.md` §3 | MCP protocol spec; gating proposed here extends `McpServer` and `McpClientBridge` traits |
| `docs/plan/specs/SPEC-tools.md` §4.1 | Progressive disclosure; gating enforces `defer_loading` and `max_injected` |
| `docs/plan/specs/SPEC-dependencies.md` §10.10–10.11 | Reference implementations (OpenHands microagents, Kilo SKILL.md) |
| `rust/crates/sera-skills/` | Implementation target for §6 and §9 Phase 2 |
| `rust/crates/sera-mcp/` | Implementation target for §7 and §9 Phase 1 |

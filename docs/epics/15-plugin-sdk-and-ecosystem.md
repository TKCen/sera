# Epic 15: Plugin SDK & Ecosystem

## Overview

SERA's open source ambition requires a stable, well-documented surface for community contributions. This epic establishes the plugin SDK (the programmatic extension interface), the `sera` CLI (for manifest validation, skill management, and local development), contributor documentation, and the agent template registry concept. Everything built here lowers the barrier for the ecosystem to grow without requiring core changes.

## Context

- See `docs/ARCHITECTURE.md` → Open Source Ecosystem (all sections)
- Plugins extend sera-core behaviour: custom skill handlers, storage providers, audit sinks, auth providers
- The plugin surface must be minimal and stable — it is a public API commitment
- Skills and agent templates are text artifacts; they contribute without writing TypeScript
- The `sera` CLI is the primary local developer tool

## Dependencies

- Epic 02 (Agent Manifest) — manifest validation (CLI validator built on this)
- Epic 06 (Skill Library) — skill format and loader (CLI skill commands built on this)

---

## Stories

### Story 15.1: Plugin interface specification

**As a** contributor
**I want** a documented, stable TypeScript interface for building sera-core plugins
**So that** I can extend SERA's behaviour without forking the core

**Acceptance Criteria:**
- [ ] Plugin interface documented in `docs/plugins/SDK.md`
- [ ] TypeScript types published at `sera-core/src/plugins/types.ts`:
  - `SkillPlugin`: `{ id, name, description, inputSchema, handler: (args, context) => Promise<SkillResult> }`
  - `StoragePlugin`: `{ id, read, write, list, delete }` (replaces or augments MemoryBlockStore)
  - `AuditSinkPlugin`: `{ id, onEvent: (event: AuditEvent) => Promise<void> }` (receives audit events)
  - `AuthPlugin`: `{ id, verify: (token: string) => Promise<AgentIdentity | null> }` (replaces JWT verification)
- [ ] `PluginContext` type: `{ logger, config, agentRegistry, circleRegistry }` — what plugins receive
- [ ] Plugins do NOT receive: Docker socket, database connection, LiteLLM client — these are internal
- [ ] Plugin interface versioned with `@sera/sdk` package version

**Technical Notes:**
- The plugin boundary is a security boundary: plugins run in the same process as sera-core but cannot access internal singletons directly
- Future: a sandboxed plugin runtime (separate process) is possible but not in scope for v1

---

### Story 15.2: Plugin registration mechanism

**As a** developer
**I want** to register a plugin with sera-core at startup
**So that** my custom skill or audit sink is available to all agents without modifying core code

**Acceptance Criteria:**
- [ ] `PluginRegistry` in sera-core loads plugins from `plugins/` directory at startup
- [ ] Plugin files export a default object conforming to a plugin interface type
- [ ] `PluginRegistry.register(plugin)` validates the plugin against the interface and registers it
- [ ] `SkillPlugin` registrations are added to `SkillRegistry` automatically
- [ ] `AuditSinkPlugin` registrations receive all audit events (in addition to the built-in DB sink)
- [ ] Plugin load errors: logged with plugin name and error, other plugins continue loading
- [ ] `GET /api/plugins` lists loaded plugins with name, type, version, status

---

### Story 15.3: `sera` CLI foundation

**As a** developer
**I want** a single `sera` CLI command with subcommands for all local development tasks
**So that** I have one tool for validating, managing, and developing SERA locally

**Acceptance Criteria:**
- [ ] `sera` CLI installable via `bun add -g @sera/cli` (package name reserved)
- [ ] Top-level commands: `sera manifest`, `sera skills`, `sera agents`, `sera version`
- [ ] `sera version` prints CLI version and compatible sera-core API version
- [ ] `--help` on every command and subcommand
- [ ] `--json` flag on read commands for machine-readable output
- [ ] Exit codes: 0 success, 1 validation error, 2 runtime error — documented
- [ ] Works offline (no running sera-core needed for `validate` commands)
- [ ] Commands that need a running sera-core: `--url` flag overrides default `http://localhost:3001`

---

### Story 15.4: `sera manifest` commands

**As a** developer
**I want** CLI commands for working with agent manifests
**So that** I can validate and inspect manifests in CI and local development

**Acceptance Criteria:**
- [ ] `sera manifest validate <path>` validates a file or directory of manifests — exit 0 valid, non-zero invalid
- [ ] `sera manifest validate --strict` also checks for deprecated fields
- [ ] `sera manifest init` interactively generates a minimal AGENT.yaml with prompted values
- [ ] `sera manifest schema` prints the JSON Schema for the current v1 manifest format
- [ ] Validation output: file path, field path, error message for each failure — one per line
- [ ] `--json` output: `{ valid: boolean, errors: [{ file, field, message }] }`

---

### Story 15.5: `sera skills` commands

**As a** developer or contributor
**I want** CLI commands for managing the skill library
**So that** I can create, validate, and install skills without a running SERA instance

**Acceptance Criteria:**
- [ ] `sera skills validate <path>` validates a skill document or directory of skills
- [ ] `sera skills list [--dir <path>]` lists skills found in a local directory
- [ ] `sera skills install <path>` copies a skill pack directory into the configured skills directory
- [ ] `sera skills init` creates a skeleton skill document with prompted id, name, category
- [ ] `sera skills schema` prints the JSON Schema for the v1 skill document format
- [ ] Validation reports: skill ID, name, error message per failure

---

### Story 15.6: Agent template format and registry stub

**As a** contributor
**I want** a format for shareable agent templates and a stub registry
**So that** the community can publish and discover agent configurations

**Acceptance Criteria:**
- [ ] Agent template format documented: an AGENT.yaml with optional `template` metadata block:
  ```yaml
  template:
    name: "@community/research-agent"
    version: "1.0.0"
    description: "A general-purpose research agent with web search and memory"
    tags: [research, web, knowledge]
  ```
- [ ] Template metadata fields optional — any AGENT.yaml is a valid template when shared
- [ ] `sera agents apply <template-path> --name my-agent --circle my-circle` renders a template with overrides and outputs a deployment-ready AGENT.yaml
- [ ] Override values merge with template: operator-provided fields take precedence
- [ ] Registry: `templates/` directory in the SERA repository as the official community template source (flat-file registry in v1, no HTTP registry server)
- [ ] `sera agents list-templates [--dir <path>]` lists available templates with descriptions

---

### Story 15.7: Contributor documentation

**As a** contributor
**I want** clear documentation on how to contribute skills, templates, plugins, and MCP server manifests
**So that** I can contribute to the SERA ecosystem without needing to understand all of Core

**Acceptance Criteria:**
- [ ] `CONTRIBUTING.md` at repo root covering: code of conduct, PR process, commit conventions, CI requirements
- [ ] `docs/contributing/SKILLS.md`: how to write a skill, the format, how to test it, how to submit a PR
- [ ] `docs/contributing/TEMPLATES.md`: how to write an agent template, naming conventions, test criteria
- [ ] `docs/contributing/MCP_SERVERS.md`: how to write an MCPServerManifest, how to test it locally
- [ ] `docs/contributing/PLUGINS.md`: how to write a plugin, the SDK, the security model, how to publish
- [ ] Each guide includes a "getting started in 5 minutes" section with a concrete worked example
- [ ] All guides reference the `sera` CLI commands for validation

---

### Story 15.8: Community MCP server SDK (`@sera/mcp-sdk`)

**As** a community developer building a SERA-compatible MCP server
**I want** an official SDK that handles the SERA MCP Extension Protocol
**So that** I can focus on tool logic rather than credential injection wire formats and acting context parsing

**Acceptance Criteria:**
- [ ] `@sera/mcp-sdk` npm package published (TypeScript, with type declarations)
- [ ] `sera-mcp` Python package published (PyPI)
- [ ] TypeScript SDK provides:
  - `SeraToolContext` type with `getCredential(name: string): string | null`, `actingContext: ActingContext`, `instanceId: string`
  - `createSeraServer(options)` — wraps the base MCP `Server` class with SERA extension handling; auto-parses `_sera` envelope and HTTP headers
  - Handler signature: `server.tool(name, schema, async (args, ctx: SeraToolContext) => result)`
- [ ] Python SDK provides equivalent: `SeraToolContext` dataclass, `sera_server` decorator
- [ ] SDK validates `ActingContext` on receipt — raises `ActingContextInvalidError` if malformed
- [ ] SDK exposes `ctx.requiresCredential(name)` which throws `CredentialUnavailableError` if the credential was not injected — consistent error that ser-core maps to the standard error code
- [ ] SDK documentation covers:
  - Getting started: minimal working tool server (< 30 lines of TypeScript)
  - Credential declaration in `tools/list` response via `x-sera` extension
  - Testing locally without a running SERA instance (mock `SeraToolContext`)
  - Manifest file (`MCPServerManifest`) that pairs with the server
- [ ] Integration test: SDK-based server registered in SERA, tool called with credential injection, credential received correctly

**Technical Notes:**
- The SDK is intentionally thin — it handles the protocol layer only, not tool discovery or transport. Developers choose their own HTTP or stdio framework.
- Package is published under the `@sera` npm org as a community-maintained package (not a core dependency of sera-core itself)

---

## Epic Summary

| Epic | Area | Priority | Key Deliverable |
|---|---|---|---|
| 01 | Infrastructure | P0 | Running stack with LiteLLM |
| 02 | Core | P0 | AGENT.yaml v1 spec + loader |
| 03 | Core | P0 | Docker sandbox + tiers |
| 04 | Core | P0 | LLM proxy + governance |
| 05 | Core | P0 | Agent runtime container |
| 06 | Core | P1 | Skill library |
| 07 | Core | P1 | MCP tool registry |
| 08 | Core | P1 | Memory + RAG |
| 09 | Core | P1 | Centrifugo messaging |
| 10 | Core | P2 | Circles + coordination patterns |
| 11 | Core | P1 | Scheduling + audit trail |
| 12 | Web | P1 | Frontend foundation |
| 13 | Web | P1 | Agent management + chat UX |
| 14 | Web | P2 | Observability + provider management |
| 15 | Ecosystem | P2 | Plugin SDK + CLI + contributor docs |

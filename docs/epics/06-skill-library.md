# Epic 06: Skill Library

## Overview

Skills are text guidance documents that shape how an agent reasons and behaves. They are not executable code — they are injected into the agent's system prompt at startup, giving the agent domain knowledge, workflows, and constraints without polluting the workspace or requiring code changes. This is SERA's alternative to OpenClaw's git-repo-clone model: selective, versioned, composable, and community-publishable.

## Context

- See `docs/ARCHITECTURE.md` → Skills vs MCP Tools, Open Source Ecosystem §3 (Skill Registry Protocol)
- Skills are Markdown files with YAML front-matter stored in `skills/{category}/{id}.md`
- They are loaded by Core at agent startup from the agent manifest's `skills:` and `skillPackages:` lists
- Skills are injected into the system prompt — they never touch the agent container's filesystem
- The skill pack format is intentionally minimal: no build step, no code, pure text

## Dependencies

- Epic 02 (Agent Manifest) — `skills` and `skillPackages` fields in manifest
- Epic 05 (Agent Runtime) — system prompt assembly before reasoning loop

---

## Stories

### Story 6.1: Skill document format specification

**As a** developer or contributor
**I want** a clear, documented format for skill documents
**So that** I can write skills that SERA loads correctly and the community has a stable contribution format

**Acceptance Criteria:**
- [ ] Skill document format documented in `docs/skills/FORMAT.md`
- [ ] Front-matter schema defined and validated:
  - `id` (required): unique kebab-case identifier
  - `name` (required): human-readable name
  - `version` (required): semver string
  - `category` (required): slash-separated path (e.g. `engineering/typescript`)
  - `tags` (optional): list of searchable strings
  - `applies-to` (optional): list of tool IDs this skill is relevant for
  - `requires` (optional): list of other skill IDs that must also be loaded
- [ ] Document body is free-form Markdown — no schema constraints on content
- [ ] JSON Schema for the front-matter (`schemas/skill-document.v1.json`) committed
- [ ] Example skills covering: a technical guidance skill, a workflow skill, a constraint-focused skill

---

### Story 6.2: Skill library loader

**As** sera-core
**I want** to load and index all skill documents from the skills directory at startup
**So that** skills are available to any agent that declares them in its manifest

**Acceptance Criteria:**
- [ ] `SkillLibrary` scans `skills/` directory recursively for `*.md` files with valid front-matter
- [ ] Each skill parsed, front-matter validated, indexed by `id`
- [ ] Duplicate `id` across files: log error, use first loaded, do not crash
- [ ] Invalid front-matter: log warning, skip file, continue loading others
- [ ] `SkillLibrary.get(id)` returns the full document (front-matter + body)
- [ ] `SkillLibrary.list(filters?)` returns all skills optionally filtered by category or tags
- [ ] Skill count and any load errors reported in sera-core startup log
- [ ] `GET /api/skills` returns all loaded skills (id, name, version, category, tags) — no body content in list view
- [ ] `GET /api/skills/:id` returns full skill document including body

---

### Story 6.3: Skill context injection at agent startup

**As an** agent
**I want** my declared skills injected into my system prompt before I start reasoning
**So that** I have the relevant domain knowledge and guidance for my role without reading files from disk

**Acceptance Criteria:**
- [ ] `SkillLoader.assemble(manifest)` resolves all skills from `skills:` and `skillPackages:` lists
- [ ] `requires` dependencies resolved recursively — if skill A requires skill B, both are loaded
- [ ] Circular `requires` detected and rejected with a clear error
- [ ] Assembled skills injected into agent system prompt as a `<skills>` XML block, each skill in its own `<skill id="..." name="...">` block
- [ ] Skills appended after the agent's `identity.role` and `identity.principles` in the system prompt
- [ ] Total context from skills capped at a configurable token limit (default: 8000 tokens) — lowest-priority skills dropped with a warning if limit exceeded
- [ ] Unknown skill IDs (declared in manifest but not found in library) log a warning and are skipped — they do not prevent agent startup

---

### Story 6.4: Skill packages

**As an** operator or contributor
**I want** to bundle related skills into a named package
**So that** agents can declare a cohesive capability set with one line rather than listing individual skills

**Acceptance Criteria:**
- [ ] Skill package format: directory containing `package.json` with `name`, `version`, `description`, `sera.type: "skill-pack"`, `sera.apiVersion: "sera/v1"`, and `skills: [id...]` array
- [ ] `SkillLibrary` scans `skill-packs/` directory and loads packages
- [ ] `GET /api/skill-packs` lists available packages
- [ ] Agent manifest: `skillPackages: [package-name]` — all skills in the package loaded
- [ ] Explicit `skills:` and `skillPackages:` are merged — duplicates deduplicated
- [ ] Package dependencies (a package requiring another package) supported via `requires: [package-name]`

---

### Story 6.5: Skill hot-reload

**As a** developer
**I want** to update a skill document and have it picked up without restarting sera-core
**So that** I can iterate on skill content quickly during development

**Acceptance Criteria:**
- [ ] `POST /api/skills/reload` triggers re-scan of the skills directory
- [ ] New skills added, removed skills removed from index, changed skills updated
- [ ] Currently running agents are not affected (they already have skills injected at startup)
- [ ] File watcher (dev mode only) that triggers reload on `.md` file changes in `skills/`
- [ ] Reload response reports: added count, removed count, updated count, any errors

---

### Story 6.6: Skill CLI (`sera skills`)

**As a** developer or contributor
**I want** CLI commands to manage skills locally
**So that** I can validate, list, and install skill packs without running SERA

**Acceptance Criteria:**
- [ ] `sera skills list` lists all skills in the local library (from `skills/` directory)
- [ ] `sera skills validate <path>` validates a skill document or skill pack against the schema — exit 0 valid, non-zero invalid
- [ ] `sera skills validate skills/` validates all skills in a directory tree
- [ ] `sera skills install <path>` copies a skill pack directory into the local library
- [ ] Clear error messages with file paths and field names for validation failures
- [ ] All commands usable in CI (documented exit codes, machine-readable `--json` output flag)

**Technical Notes:**
- The CLI is a thin wrapper over the same validation logic used in sera-core's SkillLibrary
- The `install` command is local-only in v1; a registry-based install (`sera skills install @community/pack`) is a future story

---

### Story 6.7: Skill version pinning in agent snapshots (P2 — deferred)

**As an** operator
**I want** an agent's resolved skill set pinned at spawn time alongside its resolved capabilities
**So that** upgrading a skill document does not silently change a running agent's behaviour mid-task

> **Status:** Deferred. Stub story to prevent architectural foreclosure.

**Acceptance Criteria (minimum viable, when implemented):**
- [ ] At spawn time, `resolved_config` on `agent_instances` includes a `skills` snapshot: list of `{ id, version, contentHash }` for every skill injected
- [ ] `contentHash` is SHA-256 of the skill document body (excluding front-matter) — detects content changes even when version is not bumped
- [ ] `GET /api/agents/:id` exposes `resolvedSkills` in the response
- [ ] `GET /api/agents/:id/skills/diff` compares the resolved snapshot against current library versions — returns a list of changed/added/removed skills since spawn
- [ ] Skill hot-reload (Story 6.5) does not affect already-running agents — they continue with their snapshot

**Technical Notes:**
- The snapshot is metadata only — skill content is not stored in the DB, only the hash
- This story depends on Story 6.5 (hot-reload) being stable first; hot-reload without pinning creates a race between skill updates and running agents

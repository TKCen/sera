# Epic 02: Agent Manifest & Registry

## Overview

The AGENT.yaml manifest is SERA's primary public specification — the portable, versionable artifact that defines an agent's identity, capabilities, resource limits, and communication permissions. This epic establishes the manifest format as a stable v1 spec, the loader that validates and parses it, and the DB-backed registry that tracks agent instances. This spec is intentionally designed to be community-shareable from day one.

## Context

- See `docs/ARCHITECTURE.md` → Agent Architecture, Open Source Ecosystem §1 (Stable versioned public specifications)
- The manifest format is a public API. Breaking changes require `apiVersion: sera/v2`
- Manifests live in `agents/{agent-name}/AGENT.yaml` or `agents/{agent-name}.agent.yaml`
- The DB registry persists instances derived from manifests; the manifest file is the source of truth for configuration

## Dependencies

- Epic 01 (Infrastructure Foundation) — database must be available

---

## Stories

### Story 2.1: AgentTemplate v1 specification

**As a** developer or contributor
**I want** a fully documented AgentTemplate v1 schema
**So that** I can author reusable agent blueprints that the community can publish and share

**Acceptance Criteria:**
- [ ] JSON Schema at `schemas/agent-template.v1.json`
- [ ] `apiVersion: sera/v1`, `kind: AgentTemplate` required
- [ ] `metadata` fields: `name` (kebab-case, max 63 chars), `displayName`, `icon` (emoji), `builtin` (bool, default false), `category`, `description`
- [ ] `spec` block contains: `identity`, `model`, `sandboxBoundary`, `policyRef?`, `capabilities?`, `lifecycle`, `skills?`, `skillPackages?`, `tools`, `subagents?`, `intercom?`, `resources`, `workspace?`, `memory?`
- [ ] `spec.lifecycle.mode`: `persistent | ephemeral`
- [ ] `spec.subagents.allowed[].templateRef` references another template by name (not an inline spec)
- [ ] `spec.subagents.allowed[].lifecycle` defaults to `ephemeral` — validator warns if set to `persistent`
- [ ] `spec.capabilities.seraManagement` block: per-resource-type allow lists accepting scope keywords (`own-circle`, `own-subagents`, `own`, `global`) and explicit names/patterns and `$ref` to NamedLists
- [ ] Annotated full example: `templates/example-full.template.yaml`
- [ ] Minimal example (only required fields): `templates/example-minimal.template.yaml`
- [ ] Built-in templates in `templates/builtin/` — these are bundled with sera-core and loaded before user templates
- [ ] Built-in templates are read-only — validator rejects modifications to `builtin: true` templates from API (operator must create a custom template to override)

**Technical Notes:**
- Templates are the community-publishable artifact. The schema is a public API commitment.
- `metadata.name` is the template ID — referenced by `Agent.metadata.templateRef` and `subagents.allowed[].templateRef`

---

### Story 2.1b: Agent instance v1 specification

**As an** operator or Sera (primary agent)
**I want** a documented Agent instance schema that declares overrides on a template
**So that** I can instantiate a named, configured agent from any template

**Acceptance Criteria:**
- [ ] JSON Schema at `schemas/agent-instance.v1.json`
- [ ] `apiVersion: sera/v1`, `kind: Agent` required
- [ ] `metadata` fields: `name` (unique across instance), `displayName`, `templateRef` (required — references a loaded template by name), `circle`
- [ ] `overrides` block mirrors the template `spec` structure — any field present overrides the template; absent fields inherit
- [ ] `overrides.skills.$append` and `overrides.skills.$remove` for additive/subtractive skill list modification without full replacement
- [ ] `overrides.capabilities` — can only **narrow** what the template's resolved capabilities allow; validator rejects broadening
- [ ] `overrides.capabilities.seraManagement` — same allow-list model as template; can only narrow
- [ ] Instance `overrides.sandboxBoundary` — can reference a **more restrictive** boundary than the template; cannot reference a more permissive one
- [ ] Example: `agents/example.agent.yaml` showing common override patterns
- [ ] `PATCH /api/agents/:id` updates the `overrides` block — applies immediately to next container start for persistent agents, rejected for running ephemeral agents

**Technical Notes:**
- Resolved configuration = template `spec` deep-merged with instance `overrides`, with overrides winning
- The resolution is computed at spawn time and stored as `resolved_capabilities` + `resolved_config` JSONB on the instance record
- `metadata.name` is the stable instance identifier (kebab-case, max 63 chars)

---

### Story 2.1b: NamedList, CapabilityPolicy, and SandboxBoundary as first-class resources

**As an** operator
**I want** named lists, capability policies, and sandbox boundaries managed as versioned files with their own schemas
**So that** I can define shared permission building blocks once and reference them from many agent manifests

**Acceptance Criteria:**
- [ ] **NamedList** (`kind: NamedList`) — JSON Schema at `schemas/named-list.v1.json`
  - Required: `metadata.name`, `metadata.type` (`network-allowlist | network-denylist | command-allowlist | command-denylist | secret-list`)
  - `entries`: array of strings (host patterns, glob command patterns, or secret names)
  - Entries may themselves contain `$ref: lists/{name}` to compose lists
  - Lives in `lists/{type}/{name}.yaml`
- [ ] **CapabilityPolicy** (`kind: CapabilityPolicy`) — JSON Schema at `schemas/capability-policy.v1.json`
  - Required: `metadata.name`
  - `capabilities` block covering all dimensions (see Architecture doc)
  - Allow/deny fields accept inline strings or `$ref: lists/{type}/{name}`
  - Lives in `capability-policies/{name}.yaml`
- [ ] **SandboxBoundary** (`kind: SandboxBoundary`) — JSON Schema at `schemas/sandbox-boundary.v1.json`
  - Required: `metadata.name`, `linux` block
  - `capabilities` block is the hard ceiling; same structure as CapabilityPolicy
  - Deny lists in a boundary are unconditional — they cannot be overridden by policy or manifest
  - Lives in `sandbox-boundaries/{name}.yaml`
- [ ] `$ref` resolution is recursive with cycle detection — circular references rejected with a clear error
- [ ] Built-in boundary profiles (`tier-1`, `tier-2`, `tier-3`) and the `always-denied-commands` list are bundled with sera-core and cannot be deleted (only overridden via custom boundaries)
- [ ] `GET /api/lists` — list all NamedLists
- [ ] `GET /api/capability-policies` — list all policies
- [ ] `GET /api/sandbox-boundaries` — list all boundaries
- [ ] `POST /api/agents/:id/resolve-capabilities` — returns the fully resolved effective capability set for an agent (useful for debugging and auditing)

**Technical Notes:**
- All three resource types loaded at sera-core startup before any agent manifest is processed
- Resource files watched in dev mode; `POST /api/reload` reloads all in production
- The `resolved_capabilities` JSONB column on `agent_instances` stores the output of capability resolution at spawn time — the authoritative record of what the agent was actually permitted to do

---

### Story 2.1d: CapabilityPolicy, NamedList, and SandboxBoundary — DB persistence (import-on-load)

**As** sera-core
**I want** CapabilityPolicies, NamedLists, and SandboxBoundaries persisted in the database via an import-on-load pattern
**So that** the capability resolver reads only from DB at runtime, and agent-driven orchestration can create policies programmatically without filesystem access

**Acceptance Criteria:**
- [ ] `named_lists`, `capability_policies`, and `sandbox_boundaries` tables created in the initial migration — each with: `id` (UUID), `name` (unique), `source` (`'file' | 'api'`), `spec` (JSONB), `created_at`, `updated_at`
- [ ] On sera-core startup (and on `POST /api/reload`): `ResourceImporter` scans `lists/`, `capability-policies/`, and `sandbox-boundaries/` directories, validates each file against its JSON Schema, and upserts into the respective DB table with `source: 'file'`
- [ ] File-sourced records that no longer exist on disk are marked `source: 'file-removed'` (not deleted) — active capability policies referencing them fail resolution with a clear error
- [ ] `CapabilityResolver` reads **only from DB** at resolution time — never touches the filesystem during agent spawn
- [ ] **Write endpoints** for API-created resources:
  - `POST /api/lists` — creates a NamedList with `source: 'api'`; admin/operator role required
  - `PUT /api/lists/:name` — updates an API-created NamedList; file-sourced lists return 403 (edit the file instead)
  - `DELETE /api/lists/:name` — deletes an API-created NamedList; returns 409 if referenced by an active policy
  - Same pattern for `POST/PUT/DELETE /api/capability-policies` and `POST/PUT/DELETE /api/sandbox-boundaries`
- [ ] Built-in boundaries (`tier-1`, `tier-2`, `tier-3`) imported with `source: 'builtin'`; `PUT`/`DELETE` on builtin records returns 403
- [ ] `seraManagement.policies.create` capability dimension gates API write access for agent-driven policy creation (Story 7.7)
- [ ] `GET /api/lists`, `GET /api/capability-policies`, `GET /api/sandbox-boundaries` return all records with their `source` field

**Technical Notes:**
- The dual-source model (file + api) is the key design: file-sourced resources are the GitOps/version-controlled path for operators; API-sourced resources are the programmatic path for agents and automation. Both resolve the same way at runtime.
- `$ref` resolution (NamedList referencing another NamedList) is performed against the DB, not the filesystem — all referenced names must be present in DB at resolution time

---

### Story 2.2: Manifest loader and validator

**As** sera-core
**I want** to scan agent directories, load YAML manifests, and validate them against the v1 schema
**So that** invalid manifests are rejected with clear errors before any agent is started

**Acceptance Criteria:**
- [ ] `AgentManifestLoader` scans configured agent directories recursively for `AGENT.yaml` and `*.agent.yaml` files
- [ ] Each manifest validated against JSON Schema on load — validation errors include file path and field name
- [ ] Unknown top-level fields rejected (strict mode) to catch typos
- [ ] Loader returns typed `AgentManifest` objects — no `any` types downstream
- [ ] Circular `requires` in skills detected and rejected
- [ ] `tools.denied` takes precedence over `tools.allowed` if the same tool appears in both
- [ ] Load errors logged clearly; other valid manifests still load (one bad manifest doesn't block all agents)
- [ ] Unit tests covering: valid minimal manifest, valid full manifest, missing required field, unknown field, unknown `sandboxBoundary` name, `$ref` to non-existent NamedList, inline capability override that attempts to broaden policy (rejected), circular `$ref` (rejected)

---

### Story 2.2b: Template management API

**As an** operator or agent with `seraManagement.templates.create`
**I want** to manage agent templates via the API
**So that** templates can be created, browsed, and used to instantiate agents without filesystem access

**Acceptance Criteria:**
- [ ] `GET /api/templates` — lists all templates (builtin + custom), includes `builtin` flag, `category`, `description`
- [ ] `GET /api/templates/:name` — returns full template spec
- [ ] `POST /api/templates` — creates a custom template from a request body; validates against schema; rejects `builtin: true` from API callers
- [ ] `PUT /api/templates/:name` — updates a custom template; rejects updates to builtin templates (returns 403)
- [ ] `DELETE /api/templates/:name` — deletes a custom template; rejects builtin templates; returns 409 if any active instances reference it
- [ ] `POST /api/templates/:name/instantiate` — creates an Agent instance from a template: `{ name, circle, overrides? }` → returns created agent record
- [ ] `GET /api/templates/:name/instances` — lists all Agent instances derived from this template
- [ ] Template changes do not affect already-running instances (instances hold a `resolved_config` snapshot)

---

### Story 2.2c: Sera bootstrap on first start

**As** a fresh SERA installation
**I want** the Sera primary agent to be automatically instantiated on first boot
**So that** the system is immediately usable without any manual setup

**Acceptance Criteria:**
- [ ] On sera-core startup: if `agent_instances` table is empty, auto-instantiate Sera from `templates/builtin/sera.template.yaml`
- [ ] Instance name: `sera`, circle: `default` (circle auto-created if absent)
- [ ] Bootstrap event logged: `{ action: 'bootstrap.sera_instantiated', reason: 'no_agents_found' }`
- [ ] Bootstrap does not run if any agent instance exists — idempotent
- [ ] Sera's container auto-started after instantiation
- [ ] `GET /api/system/bootstrap-status` returns: `{ bootstrapped: bool, seraInstanceId: string? }`
- [ ] If Sera template is missing (corrupted install): startup fails with a clear error, not a silent empty state

---

### Story 2.3: Agent instance DB registry

**As** sera-core
**I want** agent instances persisted in PostgreSQL
**So that** agent state survives sera-core restarts and I can query running/stopped agents

**Acceptance Criteria:**
- [ ] `agent_templates` table: `id` (UUID), `name`, `display_name`, `builtin` (bool), `category`, `spec` (JSONB), `created_at`, `updated_at`
- [ ] `agent_instances` table: `id` (UUID), `name`, `display_name`, `template_ref`, `circle`, `sandbox_boundary`, `lifecycle_mode` (`persistent|ephemeral`), `status` (`created|starting|running|stopped|error`), `parent_instance_id` (nullable — set for subagents), `container_id`, `overrides` (JSONB), `resolved_config` (JSONB — full merged config at last spawn), `resolved_capabilities` (JSONB — effective capability set at last spawn), `created_at`, `updated_at`

**Multi-user scoping (reserved):** Both `agent_templates` and `agent_instances` tables include a nullable `owner_sub TEXT` column from initial creation. In v1, this column is unpopulated and no query filtering is applied — all `operator`-role users see all resources. When resource-level scoping is activated (a future configuration flag), `owner_sub` will be populated at creation time and read queries will filter by the requesting operator's `sub`. Reserving the column now avoids a schema migration when multi-user scoping is enabled. The same pattern applies to `secrets` (Epic 16) and `delegation_tokens` (Epic 17).

- [ ] `AgentRegistry` service provides: `create(manifest)`, `get(id)`, `getByName(name)`, `list(filters)`, `updateStatus(id, status, containerId?)`, `delete(id)`
- [ ] `manifest_snapshot` stores the manifest as loaded at instance creation time (so manifest file changes don't retroactively alter running instances)
- [ ] Status transitions validated — cannot go from `stopped` to `running` without going through `starting`
- [ ] REST API: `GET /api/agents`, `GET /api/agents/:id`, `POST /api/agents`, `PUT /api/agents/:id`, `DELETE /api/agents/:id`
- [ ] `DELETE` on a running agent returns 409 Conflict with message directing caller to stop first

---

### Story 2.4: Manifest hot-reload

**As an** operator
**I want** to update an agent manifest file and have sera-core pick up the changes without a full restart
**So that** I can iterate on agent configurations during development without service interruption

**Acceptance Criteria:**
- [ ] `POST /api/agents/reload` triggers a re-scan of all manifest directories
- [ ] New manifests detected and registered; removed manifests marked as `archived`
- [ ] Changed manifests update the registry entry (but do not restart currently running instances)
- [ ] Running instances continue with the snapshot taken at their start time
- [ ] Reload errors (invalid manifest) logged and reported in the API response without affecting other manifests
- [ ] File watcher (optional, dev-mode only) that triggers reload on manifest file change

**Technical Notes:**
- File watching in production is optional; explicit `POST /reload` is the primary mechanism
- Running instance isolation from manifest changes is critical — a running agent must not be affected by a mid-run manifest edit

---

### Story 2.5: Manifest CLI validator

**As a** developer or contributor
**I want** to validate an AGENT.yaml locally before deploying it
**So that** I get fast feedback without needing a running SERA instance

**Acceptance Criteria:**
- [ ] `sera manifest validate <path>` CLI command
- [ ] Validates against the v1 JSON Schema
- [ ] Reports: valid ✓, or list of validation errors with field paths
- [ ] Exit code 0 on valid, non-zero on invalid (usable in CI pipelines)
- [ ] Installable standalone (not requiring the full sera-core to be running)
- [ ] `sera manifest validate agents/` validates all manifests in a directory tree

**Technical Notes:**
- Can be implemented as a lightweight CLI entry point that imports only the schema validation logic from sera-core
- Should be the first thing contributor documentation references

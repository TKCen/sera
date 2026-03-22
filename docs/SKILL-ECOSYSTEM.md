# SERA Skill Ecosystem Design

This document specifies the architecture for SERA's extensible skills and tools ecosystem. It covers the taxonomy, dynamic loading model, registry protocol, external ecosystem bridges, and the tool provider security model.

**Status:** Design specification. Some elements (API separation, security metadata) are implemented. Others (registry protocol, marketplace, external bridges) are future work.

---

## 1. Skills vs Tools Taxonomy

SERA enforces a strict separation between **skills** (guidance) and **tools** (execution). This is a deliberate architectural choice, not a naming convention.

### Tools — Executable Functions

Tools are **callable implementations** that produce side effects during an agent's reasoning loop. They run code, interact with filesystems, make network requests, and return structured results.

| Property | Value |
|---|---|
| Execution model | Invoked by the LLM via tool-calling protocol |
| Side effects | Yes — file I/O, shell commands, network requests |
| Security profile | **High risk** — tier-gated, allow/deny lists, capability grants |
| Storage | In-process `SkillRegistry` (builtins) or MCP server containers |
| Lifecycle | Registered at startup, hot-loaded via MCP watcher |
| Access control | `spec.tools.allowed[]` / `spec.tools.denied[]` in agent manifest |
| Versioning | Implicit (tied to MCP server image version) |

**Examples:** `file-write`, `shell-exec`, `web-search`, `knowledge-store`, `sera-core/agents.create`

**Sources:**
- **Builtin tools** — TypeScript handlers in `core/src/skills/builtins/`, registered in `SkillRegistry` at startup
- **MCP-bridged tools** — External MCP servers, bridged into `SkillRegistry` via `MCPRegistry`. Tool IDs follow the `${serverName}/${toolName}` convention
- **sera-core management tools** — Embedded MCP server exposing orchestrator operations, gated by `seraManagement` capability

### Skills — Guidance Documents

Skills are **text documents** that describe how to do something well. They are not executable. They are injected into the agent's context (system prompt or pre-task context) to shape behavior before and during the reasoning loop.

| Property | Value |
|---|---|
| Execution model | **Not executable** — injected as context text |
| Side effects | None |
| Security profile | **Low risk** — only consumes context window tokens |
| Storage | PostgreSQL `skills` table (loaded from markdown files) |
| Lifecycle | Hot-reloaded via file watcher, versioned in DB |
| Access control | `spec.skills[]` in agent manifest — freely activatable |
| Versioning | Explicit semver in frontmatter |

**Examples:** `typescript-best-practices`, `git-workflow`, `security-best-practices`

### Why the Separation Matters

1. **Security boundary:** Tools execute code inside sandboxed containers. Skills are inert text. Conflating them means treating guidance documents with the same security overhead as shell commands.

2. **Context budget:** Skills consume context window tokens. Tools consume compute time. Different budgets, different constraints.

3. **Contribution barrier:** Anyone can write a markdown skill document. Writing a secure MCP tool server requires containerization, manifest authoring, and security review.

4. **Ecosystem velocity:** A skill marketplace can accept contributions with minimal review (text only, no code execution). A tool marketplace requires image signing, vulnerability scanning, and sandbox verification.

---

## 2. Dynamic Skill Loading

### Current Model (Static)

Skills listed in `spec.skills[]` are loaded at agent startup and injected into the system prompt as a `<skills>` XML block. This is all-or-nothing: the agent gets every declared skill in its context window, whether relevant to the current task or not.

### Target Model (Dynamic)

Dynamic skill loading reduces context waste by loading skills on demand based on the task at hand.

#### 2.1 Trigger-Based Activation

Each skill declares `triggers` — keywords that indicate when the skill is relevant:

```yaml
triggers: ["typescript", "ts", "type safety", "generics"]
```

The orchestrator scans the user's message for trigger matches before injecting skills. Only skills with matching triggers are loaded. Matching is fuzzy (stemming, synonyms) not exact.

#### 2.2 Context Budget Tracking

Each skill declares `maxTokens` — an estimate of its context cost:

```yaml
maxTokens: 2000
```

The orchestrator tracks cumulative token usage across all injected skills. When the budget nears the model's context limit, lower-priority skills are dropped (FIFO or by relevance score).

**Budget formula:**
```
available = model.contextWindow - systemPrompt - messageHistory - reservedForResponse
skillBudget = min(available * 0.3, 8000)  # 30% of available, capped at 8K
```

#### 2.3 Mid-Conversation Skill Requests

The agent can request additional skills during a conversation via a `request-skill` tool call:

```json
{
  "name": "request-skill",
  "arguments": {
    "skillName": "docker-operations",
    "reason": "User asked about container networking"
  }
}
```

The orchestrator validates the budget, loads the skill content from the DB, and injects it as a system message in the next turn. The agent never sees the raw skill content in a tool response — it appears as augmented context.

#### 2.4 Skill Priority

When multiple skills match triggers, priority is determined by:
1. **Explicit declaration** — skills in `spec.skills[]` have highest priority
2. **Trigger specificity** — more specific triggers rank higher
3. **Recency** — skills matching the most recent message rank higher
4. **Token efficiency** — smaller skills (lower `maxTokens`) are preferred when budget is tight

---

## 3. Skill Registry Protocol

### 3.1 CLI Interface

```bash
# Install a skill pack from a registry
sera skills install @community/agentic-coding-pack@1.2.0

# Install from a local directory
sera skills install ./my-skills/

# List installed skills
sera skills list

# Search available skills
sera skills search "kubernetes"

# Publish a skill pack to the registry
sera skills publish ./my-skill-pack/

# Remove a skill pack
sera skills remove @community/agentic-coding-pack
```

### 3.2 Skill Pack Format

A skill pack is a directory containing:

```
my-skill-pack/
  skill-pack.yaml          # Pack manifest
  engineering/
    typescript-patterns.md  # Skill document
    error-handling.md
  operations/
    k8s-troubleshooting.md
```

**Pack manifest** (`skill-pack.yaml`):

```yaml
name: "@myorg/engineering-skills"
version: "1.2.0"
description: "Engineering guidance skills for development agents"
author: "myorg"
license: "MIT"
sera:
  type: skill-pack
  apiVersion: sera/v1
  minCoreVersion: "0.5.0"
skills:
  - typescript-patterns
  - error-handling
  - k8s-troubleshooting
```

### 3.3 Version Resolution

- Agent manifests pin to version ranges: `skills: ["typescript-best-practices@^1.0.0"]`
- The registry resolves to the latest matching version
- Multiple versions can coexist in the DB (keyed by `name + version`)
- The `conflicts` field prevents incompatible skills from loading together

### 3.4 Registry Backend

Phase 1 (v0): Flat-file registry — skills loaded from local `skills/` directories and `SKILL_PACK_DIRS` env var. No remote fetch.

Phase 2 (v1): HTTP registry server with:
- `GET /v1/skills?q=...` — search skills
- `GET /v1/skills/:name/versions` — list versions
- `GET /v1/skills/:name/:version` — download pack tarball
- `POST /v1/skills` — publish (authenticated)

Phase 3 (v2): Federated registries — organizations host private registries, public registry for community skills.

---

## 4. External Ecosystem Bridge

### 4.1 Adapter Pattern

External skill sources are imported through typed adapters:

```typescript
interface SkillSourceAdapter {
  name: string;
  fetch(query: string): Promise<ExternalSkill[]>;
  import(skill: ExternalSkill): Promise<SkillDocument>;
}
```

Each adapter maps the external source's format to SERA's `SkillDocument` schema.

### 4.2 ClawHub / OpenClaw Bridge

[ClawHub](https://clawhub.ai/) and the OpenClaw ecosystem package agent guidance as `.claw` files — essentially markdown with YAML frontmatter, similar to SERA's skill format.

**Mapping:**

| ClawHub Field | SERA Field | Notes |
|---|---|---|
| `name` | `name` | Direct map |
| `description` | `description` | Direct map |
| `version` | `version` | Direct map or default `1.0.0` |
| `tags` | `tags` | Direct map |
| `content` (markdown body) | `content` | Direct map |
| `category` | `category` | Map to SERA's category taxonomy |
| — | `triggers` | Auto-derived from tags + content keywords |
| — | `maxTokens` | Estimated from content length |

**Import flow:**

```bash
# Import a single skill from ClawHub
sera skills import --source clawhub "agentic-coding-guide"

# Import with trust level
sera skills import --source clawhub --trust unverified "community-skill"
```

**Trust levels for imported skills:**

| Level | Meaning | Review Required |
|---|---|---|
| `unverified` | Imported from external source, no review | Operator accepts risk |
| `reviewed` | Operator has read and approved the content | Manual sign-off |
| `verified` | Cryptographically signed by a trusted publisher | Signature validation |

### 4.3 Git Repository Import

For sources that publish skills as git repositories (e.g., OpenClaw repos):

```bash
sera skills import --source git https://github.com/org/skill-pack.git
```

The importer clones the repo, scans for `.md` files with valid frontmatter, and upserts them into the skill library. Unlike OpenClaw's model of cloning repos into agent workspaces, SERA extracts only the skill documents — no code, no workspace pollution.

---

## 5. Tool Provider Security Model

### 5.1 MCP Server Sandboxing

External MCP tool servers run as sandboxed Docker containers on the `agent_net` network, managed by `MCPServerManager`. They follow the same isolation model as agent containers.

**MCP Server Manifest:**

```yaml
apiVersion: sera/v1
kind: SkillProvider
metadata:
  name: github-mcp
  description: "GitHub API tool provider"
image: ghcr.io/modelcontextprotocol/servers/github:latest
transport: stdio
network:
  allowlist:
    - api.github.com
secrets:
  - name: GITHUB_TOKEN
    from: sera-vault
```

### 5.2 Trust Levels for Tool Providers

| Level | Source | Security | Review |
|---|---|---|---|
| `builtin` | Shipped with SERA | Full trust, no sandbox overhead | Core team maintained |
| `verified` | Signed by known publisher | Standard sandbox, image hash verified | Publisher identity verified |
| `community` | Community-contributed | Strict sandbox, network denylist | Operator assumes risk |
| `untrusted` | Unknown source | Maximum isolation, no network by default | Manual per-tool approval |

### 5.3 Capability Requirements

Tools declare their capability requirements in the MCP manifest. The agent's resolved capabilities must satisfy all requirements:

```yaml
capabilities:
  required:
    - network.outbound    # Needs outbound network access
    - filesystem.write    # Needs workspace write access
  optional:
    - seraManagement      # Can manage other agents if granted
```

An agent at tier 3 (most restricted) cannot use tools requiring `network.outbound`. The tool picker in the UI shows warnings when a tool's requirements exceed the agent's tier.

### 5.4 Egress Proxy Enforcement

All MCP container network traffic routes through a reverse proxy that enforces the `network.allowlist` from the manifest. Requests to non-allowlisted hosts are blocked and logged in the audit trail.

### 5.5 Runtime Security Properties

| Property | Enforcement |
|---|---|
| Read-only rootfs | `readOnlyRootfs: true` on all MCP containers |
| No privilege escalation | `noNewPrivileges: true` |
| Dropped capabilities | All Linux capabilities dropped unless explicitly granted |
| Resource limits | CPU and memory limits from manifest |
| Ephemeral storage | Scratch space via tmpfs, cleared on container stop |
| Secret injection | Environment variables, never mounted files |

---

## 6. Future: Skill Marketplace

### 6.1 Discovery

A searchable web interface and CLI for finding skills:

```bash
sera skills search "kubernetes deployment" --category operations
```

Results ranked by:
- Relevance to search query
- Download count / popularity
- Verified publisher status
- Compatibility with installed SERA version

### 6.2 Quality Signals

| Signal | Source | Weight |
|---|---|---|
| Verified publisher | GPG signature + publisher registry | High |
| Download count | Registry analytics | Medium |
| Operator ratings | 1-5 stars + text review | Medium |
| Automated lint | Frontmatter validation, token budget check | Low |
| Freshness | Last update timestamp | Low |

### 6.3 Dependency Resolution

Skill packs can declare dependencies on other packs:

```yaml
dependencies:
  "@sera/core-engineering": "^1.0.0"
  "@sera/git-workflow": "^2.0.0"
```

The resolver uses semver range matching, topological sorting, and conflict detection (via `conflicts` field) to produce a valid skill set.

### 6.4 Publication Flow

```bash
# Validate pack format
sera skills validate ./my-pack/

# Dry-run publish
sera skills publish --dry-run ./my-pack/

# Publish to registry
sera skills publish ./my-pack/ --registry https://registry.sera.dev
```

Publication requires:
1. Valid `skill-pack.yaml` manifest
2. All referenced skill documents present with valid frontmatter
3. No circular dependencies
4. Publisher authentication (API key or OIDC)
5. Optional: GPG signature for verified publisher status

---

## Appendix: API Endpoints

### Tools API (`GET /api/tools`)

Returns executable tools with security metadata:

```json
[
  {
    "id": "file-write",
    "description": "Write content to a file, creating directories as needed.",
    "parameters": [...],
    "source": "builtin",
    "minTier": 1,
    "usedBy": ["architect"]
  },
  {
    "id": "sera-core/agents.create",
    "description": "Create a new agent instance",
    "parameters": [...],
    "source": "mcp",
    "server": "sera-core",
    "minTier": 2,
    "capabilityRequired": "seraManagement",
    "usedBy": []
  }
]
```

### Skills API (`GET /api/skills`)

Returns guidance skills with content metadata:

```json
[
  {
    "id": "typescript-best-practices",
    "name": "typescript-best-practices",
    "version": "1.0.0",
    "description": "Guidance on writing clean, safe, and idiomatic TypeScript.",
    "category": "engineering/typescript",
    "tags": ["typescript", "best-practices", "guidance"],
    "triggers": ["typescript", "ts", "coding"],
    "maxTokens": 2000,
    "source": "bundled",
    "usedBy": ["developer"]
  }
]
```

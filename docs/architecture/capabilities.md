# Capability & Permission Model

SERA uses a three-layer permission model. Every access decision is the intersection of all three layers — the most restrictive wins.

## The Three Layers

```
┌─────────────────────────────────────────────┐
│  Layer 3: SandboxBoundary (hard ceiling)    │  ← operator-defined
├─────────────────────────────────────────────┤
│  Layer 2: CapabilityPolicy (grant set)      │  ← named policy
├─────────────────────────────────────────────┤
│  Layer 1: Manifest inline (narrowing only)  │  ← agent-specific
└─────────────────────────────────────────────┘

Effective = Boundary ∩ Policy ∩ ManifestOverride
Deny always beats Allow at every layer.
```

### SandboxBoundary — Hard Ceiling

Defines the maximum capabilities any agent using this boundary can ever have. Stored in `sandbox-boundaries/`.

SERA ships with three tiers:

| Tier       | Network               | Shell | Subagents | Root     | Use case              |
| ---------- | --------------------- | ----- | --------- | -------- | --------------------- |
| **tier-1** | Read-only, air-gapped | No    | No        | No       | Research agents       |
| **tier-2** | Filtered outbound     | Yes   | Yes       | No       | Development agents    |
| **tier-3** | Filtered outbound     | Yes   | Yes       | Optional | Privileged operations |

Operators can define custom boundaries (e.g., `ci-runner`, `air-gapped`, `read-only-analyst`).

### CapabilityPolicy — Grant Set

Defines what an agent is allowed to do within its boundary ceiling. Stored in `capability-policies/`.

```yaml
kind: CapabilityPolicy
metadata:
  name: typescript-developer
capabilities:
  filesystem:
    read: true
    write: true
    scope: ['/workspace/**']
  network:
    outbound:
      allow:
        - $ref: lists/npm-registry
        - $ref: lists/github-apis
  exec:
    shell: true
    commands:
      allow:
        - $ref: lists/standard-dev-tools
      deny:
        - $ref: lists/always-denied-commands
  llm:
    budget:
      hourly: 100000
      daily: 500000
```

### Manifest Inline — Narrowing Only

Agents can further restrict (never broaden) their capabilities:

```yaml
capabilities:
  network:
    outbound:
      allow:
        - $ref: lists/github-apis
        # npm-registry from policy is dropped — narrower
  docker:
    spawnSubagents: false
```

## NamedLists

Any allow or deny list can reference a `NamedList` instead of inlining values. Update one list and every referencing policy picks up the change.

```yaml
kind: NamedList
metadata:
  name: github-apis
  type: network-allowlist
entries:
  - 'api.github.com'
  - 'raw.githubusercontent.com'
```

Lists can compose other lists via `$ref`.

## Capability Dimensions

| Dimension                  | Controls                                  |
| -------------------------- | ----------------------------------------- |
| `filesystem`               | read/write/delete flags, path scope globs |
| `network.outbound`         | allow/deny host lists (supports `$ref`)   |
| `network.maxBandwidthKbps` | Per-agent bandwidth limit                 |
| `exec.shell`               | Shell access toggle                       |
| `exec.commands`            | Allow/deny command patterns               |
| `llm.models`               | Allowed model name patterns               |
| `llm.budget`               | Hourly/daily token limits                 |
| `memory`                   | Read/write/delete, namespace scopes       |
| `intercom`                 | Publish/subscribe channel patterns        |
| `docker.spawnSubagents`    | Subagent spawning permission              |
| `secrets.access`           | Named secrets the agent may receive       |
| `seraManagement`           | SERA instance management operations       |

## Dynamic Permission Grants

When an agent encounters a resource outside its capability set, it can request a runtime grant:

| Grant Type   | Scope                 | Persistence                   |
| ------------ | --------------------- | ----------------------------- |
| `one-time`   | Single operation      | Nothing stored                |
| `session`    | Remainder of this run | In-memory only                |
| `persistent` | All future runs       | Stored in DB, optional expiry |

The operator sees a prompt in the dashboard and approves or denies. The agent's tool call blocks (with timeout) until the decision arrives.

## Resolution at Spawn Time

```
For each capability dimension:
  1. Start with SandboxBoundary ceiling
  2. Intersect with CapabilityPolicy grants
  3. Apply manifest inline narrowing
  4. Apply global deny lists (unconditional)

  Allow wins only if: allowed by boundary AND policy AND not denied at any layer
```

# capability-policies/

This directory contains `CapabilityPolicy` definitions. A capability policy is
a named grant set that controls what an agent is allowed to do within the hard
ceiling set by its `sandboxBoundary`. Policies are loaded by `ResourceImporter`
at startup and stored in the `capability_policies` DB table.

## How to reference a policy from an agent manifest

```yaml
# agents/my-agent/AGENT.yaml
apiVersion: sera/v1
kind: Agent
metadata:
  name: my-agent
sandboxBoundary: tier-2        # hard ceiling — must be >= policy requirements
policyRef: sandboxed-coder     # base grant set from capability-policies/
```

Inline `capabilities:` blocks in the manifest can only *narrow* the policy —
they can never broaden beyond what the policy grants.

## Starter policies

### `read-only.yaml`

Grants filesystem read access within `/workspace/**` only. No writes, no shell
execution, no outbound network. Budget is capped at 50k tokens/hour. Designed
for summarizer, analyzer, and code-review agents that must inspect content but
must never mutate state or exfiltrate data. Pair with `sandboxBoundary: tier-1`.

### `sandboxed-coder.yaml`

Grants read/write/delete within `/workspace/**` and `/tmp/**`, plus shell
execution of common build and test commands (`cargo`, `npm`, `bun`, `vitest`,
`pytest`, etc.). Outbound network is blocked — LLM provider traffic is handled
transparently by the SERA LiteLLM gateway. Git push and remote operations are
explicitly denied. Budget is 100k tokens/hour. Suitable for automated coding
and test-running agents operating inside a tier-2 Docker sandbox.

### `full-dev.yaml`

Grants the full set of capabilities expected by a senior developer agent:
read/write/delete anywhere under `/workspace`, `/tmp`, and `/home`; shell
execution including `git`, `docker build/run`, and all common toolchains;
outbound network to GitHub, npm, PyPI, and crates.io (cloud metadata endpoints
denied); sub-agent spawning (max 5); and access to `GITHUB_TOKEN` and
`NPM_TOKEN` secrets. Budget is 200k tokens/hour / 1M tokens/day. Must be
paired with `sandboxBoundary: tier-2`. Do not use for untrusted agents.

## Schema

Policies follow `schemas/capability-policy.v1.json` (epic 02). Required fields:

| Field | Type | Description |
|---|---|---|
| `apiVersion` | `sera/v1` | Fixed |
| `kind` | `CapabilityPolicy` | Fixed |
| `metadata.name` | string | Unique policy name |
| `capabilities` | object | Grant set (see architecture doc) |

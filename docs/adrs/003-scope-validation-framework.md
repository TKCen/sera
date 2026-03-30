# ADR-003: Scope Validation Framework

**Status:** Proposed
**Date:** 2026-03-30
**Related:** Epic 17 (Agent Identity & Delegation), Stories 17.5-17.9

## Context

Agents can delegate tasks to sub-agents and access resources across circles. The delegation token system exists (Epic 17 Stories 17.1-17.3) but scope validation — ensuring an agent only accesses resources within its authorized boundary — is not enforced.

## Current State

| Component | Status |
|---|---|
| `ActingContext` enum | ✅ Defined (autonomous, delegated-from-operator, delegated-from-agent) |
| Delegation token issuance | ✅ `DelegationTokenService` creates signed tokens |
| Token verification | ✅ JWT validation in auth middleware |
| **Scope enforcement** | ❌ Not implemented |
| **Resource ownership checks** | ❌ Not implemented |
| **Delegation chain audit** | ⚠️ Partial (tokens recorded, chain not validated) |

## Problem

Without scope validation, a delegated agent can:
- Access memory blocks from any agent, not just those in its circle
- Read/write files in any workspace if granted filesystem access
- Call MCP tools that affect resources outside its authorization boundary
- Spawn sub-agents without scope propagation

## Canonical Design (Epic 17)

Story 17.5 defines a `CredentialResolver` that:
1. Checks the acting context (who initiated the action)
2. Resolves the effective scope (what resources are accessible)
3. Validates that the requested resource is within scope
4. Propagates reduced scope to sub-agents (principle of least privilege)

Story 17.9 defines scope as:
- **Personal**: agent's own resources only
- **Circle**: resources in the agent's primary circle
- **Global**: system-wide resources (admin only)
- Scope narrows down the delegation chain: operator → agent → sub-agent

## Recommended Approach

### 1. Resource scope model

Every API operation that accesses agent-specific resources should check:
```typescript
interface ScopeCheck {
  actorId: string;         // who is acting
  actorScope: Scope;       // what scope they have (from delegation chain)
  resourceOwnerId: string; // who owns the resource
  resourceCircle?: string; // which circle the resource belongs to
}
```

### 2. Scope resolution chain

```
Operator (full scope for their role)
  → delegates to Agent A (scope: circle "engineering")
    → delegates to Sub-Agent B (scope: personal only — narrowed)
```

Each delegation narrows or maintains scope — never widens.

### 3. Enforcement points

| Endpoint | Check |
|---|---|
| `GET /api/memory/:agentId/blocks` | Actor must own the agent or be in the same circle |
| `POST /v1/tools/proxy` | Actor's scope must include the target resource |
| `POST /api/agents/spawn-ephemeral` | Sub-agent scope ≤ parent scope |
| MCP tool calls | Scope from delegation token propagated to MCP server |

### 4. Implementation order

1. Add `scope` field to delegation tokens
2. Add `ScopeValidator` service that checks actor vs resource
3. Wire into memory routes, tool proxy, and agent spawn
4. Add scope propagation to sub-agent creation
5. Audit log scope violations

## Consequences

### Positive
- Agents can't access resources outside their authorization
- Delegation chains are auditable and scope-narrowing
- Circle boundaries are enforced, not just declared

### Negative
- Every resource access adds a scope check (small overhead)
- Existing agents with implicit full access may break (need migration)
- Complexity in the auth middleware increases

# Epic 17: Agent Identity & Delegation

## Overview

Agents that interact with external systems need more than an internal JWT — they need an identity model that is meaningful to those systems and an authority model that is honest about who or what is acting. This epic introduces three distinct acting contexts (agent-as-principal, agent-on-behalf-of-user, agent-on-behalf-of-agent), a delegation token system with interactive HitL approval, per-agent service identities separate from shared secrets, and credential resolution logic that makes the acting context unambiguous at every tool call. The audit trail extension ensures the full delegation chain is always attributable.

## Context

- See `docs/ARCHITECTURE.md` → Authentication, Secrets Management, Capability & Permission Model
- Complements Epic 16 (Authentication & Secrets) — secrets are named credentials; this epic introduces *who holds authority over them and in what context*
- Parallels Story 3.9 (permission requests for capabilities) — this epic introduces the same HitL pattern for credential delegation
- Three acting contexts:
  - **Autonomous**: agent acts as its own principal using its own service identity
  - **Delegated-from-operator**: agent acts on behalf of an operator; authority is the operator's, scoped to what they chose to delegate
  - **Delegated-from-agent**: parent agent passes a subset of its own authority to a subagent; child cannot exceed parent's scope

## Dependencies

- Epic 16 (Authentication & Secrets) — `SecretsProvider`, operator identity, `OperatorIdentity`
- Epic 03 (Docker Sandbox) — container spawn, `SERA_IDENTITY_TOKEN` JWT, permission request flow (Story 3.9)
- Epic 07 (MCP Tool Registry) — MCP server containers that execute external calls
- Epic 11 (Scheduling & Audit) — audit trail records that need delegation chain enrichment

---

## Stories

### Story 17.1: ActingContext type system

**As** sera-core
**I want** a formal ActingContext type that travels with every tool execution and audit record
**So that** every external action carries an unambiguous record of who holds the authority, who is performing the action, and how the authority was acquired

**Acceptance Criteria:**
- [ ] `ActingContext` TypeScript type defined in shared types package:
  ```typescript
  interface ActingContext {
    principal: {
      type: 'operator' | 'agent'
      id: string            // operatorSub or agentId
      name: string          // email or agentName
      authMethod: 'oidc' | 'api-key' | 'agent-jwt' | 'delegation'
    }
    actor: {
      agentId: string
      agentName: string
      instanceId: string
    }
    delegationChain: DelegationLink[]  // empty = agent acting as own principal
    delegationTokenId?: string         // set when using an active delegation token
  }

  interface DelegationLink {
    delegatorType: 'operator' | 'agent'
    delegatorId: string      // operatorSub or agentId
    delegatorName: string
    scope: DelegationScope
    grantType: 'one-time' | 'session' | 'persistent'
    issuedAt: string         // ISO8601
    expiresAt?: string
  }

  interface DelegationScope {
    service: string          // e.g. 'github', 'google-calendar', '*'
    permissions: string[]    // e.g. ['repo:read', 'issues:write'] or ['*']
    resourceConstraints?: Record<string, string[]>  // e.g. { repos: ['org/repo'] }
  }
  ```
- [ ] `ActingContext` built at request time by `ActingContextBuilder`:
  - Autonomous context: `principal = actor = agent`, empty `delegationChain`, built from the agent JWT
  - Operator-delegated context: built from the active delegation token — principal is the operator, actor is the agent
  - Agent-delegated context: built from a child delegation token — chain carries all links from operator → parent → child
- [ ] `ActingContext` available throughout the request lifecycle via request context (parallel to `OperatorIdentity` from Epic 16.4)
- [ ] `ActingContext` validated: `delegationChain` depth checked against `DELEGATION_MAX_CHAIN_DEPTH` (default: 5); chain integrity checked (each link's `delegatorId` matches the previous link's `actor.agentId` or the initial operator)
- [ ] `ActingContext` serialised to audit trail on every external-action event (Story 17.8)
- [ ] Unit tests: build context for all three cases; detect broken chain; detect depth violation

---

### Story 17.2: Agent service identity registry

**As an** operator
**I want** to register dedicated external service identities for specific agents
**So that** agents can have their own accounts on external systems rather than sharing operator credentials

**Acceptance Criteria:**
- [ ] `agent_service_identities` table:
  ```sql
  CREATE TABLE agent_service_identities (
    id              UUID PRIMARY KEY,
    agent_scope     TEXT NOT NULL,       -- agent template name OR agent instance ID (UUID) OR '*' (any agent)
    service         TEXT NOT NULL,       -- e.g. 'github', 'slack', 'jira', 'custom'
    external_id     TEXT,                -- e.g. GitHub bot user login, Slack bot ID
    display_name    TEXT,
    credential_secret_name TEXT NOT NULL, -- reference to a secret in SecretsProvider
    scopes          TEXT[],              -- what this identity can do on the external service
    created_at      TIMESTAMPTZ,
    rotated_at      TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
  );
  ```
- [ ] `POST /api/agents/:id/service-identities` registers a new service identity for an agent — admin/operator role required
- [ ] `GET /api/agents/:id/service-identities` lists registered identities (credential value never returned, only metadata)
- [ ] `DELETE /api/agents/:id/service-identities/:identityId` revokes an identity
- [ ] `POST /api/agents/:id/service-identities/:identityId/rotate` updates the underlying secret reference (e.g. after rotating a bot token)
- [ ] `agent_scope` resolution: instance-level identity takes precedence over template-level; template-level over `*`
- [ ] Service identities visible in agent manifest `GET /api/agents/:id` under `serviceIdentities: [{ service, externalId, displayName, scopes }]` — no credential values
- [ ] Service identities are **not** injected at container spawn as env vars — they are resolved at tool execution time via `CredentialResolver` (Story 17.5)

**Technical Notes:**
- The secret referenced by `credential_secret_name` is a standard entry in the secrets table (Epic 16.8) — service identity is the governance wrapper around it, not a separate storage mechanism
- This separates lifecycle concerns: the secret is rotated independently; the service identity record tracks external metadata (`external_id`, `scopes`)

---

### Story 17.3: Operator-to-agent delegation (pre-configured)

**As an** operator
**I want** to pre-configure a scoped delegation that allows a specific agent to act on my behalf for a specific service
**So that** I can authorise the agent before it begins its task without waiting for a runtime request

**Acceptance Criteria:**
- [ ] `delegation_tokens` table:
  ```sql
  CREATE TABLE delegation_tokens (
    id                   UUID PRIMARY KEY,
    principal_type       TEXT NOT NULL,  -- 'operator'
    principal_id         TEXT NOT NULL,  -- operatorSub
    principal_name       TEXT NOT NULL,  -- email
    actor_agent_id       TEXT NOT NULL,  -- agentId (template or instance)
    actor_instance_id    UUID,           -- set if delegation is instance-scoped
    scope                JSONB NOT NULL, -- DelegationScope
    grant_type           TEXT NOT NULL,  -- 'session' | 'persistent'
    credential_secret_name TEXT NOT NULL,-- the secret being delegated
    issued_at            TIMESTAMPTZ,
    expires_at           TIMESTAMPTZ,
    revoked_at           TIMESTAMPTZ,
    last_used_at         TIMESTAMPTZ,
    use_count            INTEGER DEFAULT 0,
    parent_delegation_id UUID            -- set when agent delegates to subagent
  );
  ```
- [ ] `POST /api/delegation` creates a delegation — requires OIDC-authenticated operator; operator can only delegate secrets they own or have explicit delegation rights on
- [ ] Request body: `{ agentId, service, permissions, resourceConstraints?, credentialSecretName, grantType, expiresAt?, instanceScoped? }`
- [ ] `GET /api/delegation` lists the authenticated operator's active delegations
- [ ] `DELETE /api/delegation/:id` revokes a delegation immediately; sets `revoked_at`
- [ ] `GET /api/agents/:id/delegations` lists active inbound delegations for an agent — admin/operator role required; no credential values returned
- [ ] Scope constraint validation: operator cannot delegate permissions broader than what the referenced secret can provide — enforced via `scopes` on `agent_service_identities` or explicit check
- [ ] Delegation creation recorded in audit trail: `{ action: 'delegation.created', principalId, actorAgentId, service, grantType }`

---

### Story 17.4: Interactive delegation request (HitL)

**As an** agent mid-task
**I want** to request that an operator delegate a scoped credential to me at runtime
**So that** I can ask for the specific authority I need to complete a task rather than failing silently

**Acceptance Criteria:**
- [ ] `POST /api/agents/:id/delegation-request` accepts from an authenticated agent (JWT):
  `{ service, requestedPermissions, resourceConstraints?, reason, preferredGrantType?: 'one-time'|'session'|'persistent' }`
- [ ] Request published to Centrifugo `system.delegation-requests`: `{ requestId, agentId, agentName, service, requestedPermissions, resourceConstraints, reason, preferredGrantType, requestedAt }`
- [ ] Request held pending operator decision — agent call blocks with configurable timeout (default: 5 min; same as Story 3.9)
- [ ] `POST /api/delegation-requests/:requestId/decision`:
  - `decision: 'grant'`: operator selects which of their secrets to delegate + optional scope narrowing + grant type + optional `expiresAt`
  - `decision: 'deny'`: optional reason
- [ ] On **grant**: sera-core creates a `delegation_tokens` record scoped to the requesting agent instance; returns `{ granted: true, delegationTokenId }` to the waiting agent
- [ ] On **deny**: `{ granted: false, reason? }` returned; agent handles gracefully
- [ ] On timeout: auto-deny; operator notified via Centrifugo that request expired
- [ ] `GET /api/delegation-requests` lists pending requests — filterable by agent; requires operator role
- [ ] UI delegation request dialog (Epic 13/14) shows: requesting agent, service, requested permissions, reason, and "You are granting as: alice@example.com with secret: [secret name]"
- [ ] Interactive delegation records in audit trail: `{ action: 'delegation.requested', agentId, service, reason }` and `{ action: 'delegation.granted|denied', operatorSub, agentId, service, grantType }`

**Technical Notes:**
- The operator selects a secret to back the delegation — they do not paste credentials into the request dialog; credentials must already be stored in the SecretsProvider (Epic 16.8)
- One-time grants: delegation token `revoked_at` is set immediately after first use by `CredentialResolver`

---

### Story 17.5: Tool credential resolution

**As** the agent runtime and sera-core MCP handler
**I want** a `CredentialResolver` that selects the correct credential for a tool call based on the current acting context
**So that** credential selection is deterministic, auditable, and context-aware

**Acceptance Criteria:**
- [ ] `CredentialResolver.resolve(service, agentContext, actingContext)` returns `{ value: string, sourceType: 'delegation' | 'service-identity' | 'secret', sourceId: string } | null`
- [ ] Resolution order (first match wins, `null` = no credential available):
  1. Active delegation token in `actingContext.delegationTokenId` — if the token's `scope.service` matches and is not expired/revoked
  2. Agent service identity matching `service` for this agent instance or template (Story 17.2)
  3. Named secret in SecretsProvider with `allowedAgents` including this agent and a tag/naming convention matching `service` (Story 16.8)
  4. No match → return `null`
- [ ] Resolution logged at DEBUG level: which source was used, delegationTokenId or secretName, not the value
- [ ] On resolution via delegation token: increment `use_count`; if `grant_type: 'one-time'` → set `revoked_at` immediately after returning the value
- [ ] `CredentialResolver` never returns a credential from a revoked or expired delegation token — expiry and revocation checked on every resolve call (not cached)
- [ ] `CredentialResolver` is the **only** code path that reads secret values for external tool use — direct secret reads outside this resolver require explicit justification in code review
- [ ] Credential resolution result (without value) included in audit record for external tool calls

**Technical Notes:**
- The resolver is in sera-core, not in the agent container — agent containers never directly hold delegation tokens; credential values are passed into the tool call by sera-core at execution time
- For MCP tools: sera-core injects the resolved credential as an env var into the specific tool invocation, not at container spawn time (overrides the spawn-time env for that call)

---

### Story 17.6: Agent-to-subagent credential delegation

**As** a parent agent with an active delegation token
**I want** to pass a scoped subset of my delegated authority to a subagent I spawn
**So that** the subagent can act with delegated authority while being further constrained below my own scope

**Acceptance Criteria:**
- [ ] `sera-core/agents.create` MCP tool (Story 7.7) accepts an optional `delegations` parameter: list of `{ delegationTokenId, narrowedScope? }` from the parent's active delegation tokens
- [ ] Scope narrowing validation: `narrowedScope.permissions` must be a subset of the parent delegation's `scope.permissions`; `narrowedScope.resourceConstraints` must further constrain (not expand) the parent's constraints
- [ ] Attempting to pass a scope broader than the parent holds → `CapabilityEscalationError`, spawn rejected
- [ ] sera-core creates new child delegation tokens with `parent_delegation_id` set to the parent's token ID; child tokens share the same `credential_secret_name` as the parent but carry the narrowed scope
- [ ] Child delegation tokens are tied to the child agent's `instanceId` — they cannot be transferred
- [ ] On parent agent stop: all child delegation tokens derived from the parent's session grants are revoked immediately (`revoked_at` set)
- [ ] `delegation_tokens` record carries the full lineage via `parent_delegation_id` chain — fully traversable
- [ ] `GET /api/delegation/:id/children` returns all derived child tokens (admin role)
- [ ] Audit trail: `{ action: 'delegation.derived', parentDelegationId, childDelegationId, childAgentId, narrowedScope }`

---

### Story 17.7: Delegation revocation and lifecycle management

**As an** operator
**I want** full lifecycle control over delegations — including cascade revocation
**So that** I can immediately withdraw authority I've granted, with predictable effects on active agents

**Acceptance Criteria:**
- [ ] `DELETE /api/delegation/:id` revokes the delegation token: sets `revoked_at`
- [ ] `DELETE /api/delegation/:id?cascade=true` revokes the token and all child tokens derived from it (via `parent_delegation_id` chain)
- [ ] Revocation takes effect immediately — `CredentialResolver` checks `revoked_at` on every call, no TTL caching
- [ ] On revocation of an active session delegation: affected agent receives a `system.delegation-revoked` event via Centrifugo; agent runtime logs a `reflect` thought: `"Delegation for service [X] was revoked by the operator"`
- [ ] Expired delegations (`expires_at` in the past): treated as revoked by `CredentialResolver`; background job marks them `revoked_at = expires_at` for clean record-keeping
- [ ] `GET /api/delegation` includes `status: active | expired | revoked` field
- [ ] `GET /api/delegation?status=active` filters to currently usable delegations only
- [ ] Operator dashboard (Epic 14) shows active delegations per agent with revoke button
- [ ] Revocation events recorded in audit trail: `{ action: 'delegation.revoked', delegationId, revokedBy, cascade, childTokensRevoked: N }`

---

### Story 17.8: Audit trail delegation chain

**As an** auditor or operator
**I want** every external-action audit record to carry the full delegation chain
**So that** I can always answer "who ultimately authorised this action and with what scope"

**Acceptance Criteria:**
- [ ] All audit trail records for external tool calls enriched with `actingContext`: `{ principal, actor, delegationChain, delegationTokenId? }`
- [ ] `actingContext.principal` distinguishes the three cases clearly:
  - Autonomous: `{ type: 'agent', id: agentId, name: agentName, authMethod: 'agent-jwt' }`
  - Operator-delegated: `{ type: 'operator', id: operatorSub, name: email, authMethod: 'oidc'|'api-key' }`
  - Agent-delegated: `{ type: 'agent', id: parentAgentId, name: parentAgentName, authMethod: 'delegation' }`
- [ ] `delegationChain` array carries all links — empty for autonomous, one link for operator-to-agent, N links for deep chains
- [ ] `GET /api/audit?principalId=` filters by the authority holder (operator or parent agent), not just the acting agent
- [ ] `GET /api/audit?delegationId=` returns all actions taken under a specific delegation token
- [ ] Audit records for denied credential resolutions (Story 17.5 returning `null`) also recorded: `{ action: 'credential.resolution.denied', service, agentId, reason: 'no_matching_credential' }`
- [ ] Audit query for a specific delegation: `GET /api/delegation/:id/audit` — returns all audit records attributed to that delegation token

**Technical Notes:**
- The `delegationChain` is denormalised into the audit record at write time — not a foreign key reference. This ensures the audit record is immutable and self-contained even after a delegation token is later deleted.
- The Merkle hash-chain (Epic 11) covers the full `actingContext` field — tampering with the delegation attribution is detectable

---

### Story 17.9: Acting context in MCP tool execution

**As** sera-core
**I want** MCP tool calls to carry and forward the calling agent's acting context
**So that** MCP servers and their external calls can use the correct credentials and produce attributable audit records

**Acceptance Criteria:**
- [ ] When sera-core proxies a tool call to an MCP server container (Story 7.3), the request includes the `actingContext` in a header or request field: `X-Sera-Acting-Context: {base64-encoded JSON}`
- [ ] `CredentialResolver` called before each MCP tool invocation: resolved credential (if any) injected as an env var override for the specific execution — not stored in the container's base env
- [ ] MCP server containers **cannot** access `CredentialResolver` directly — only sera-core calls it; the resolved value is injected, the resolution logic stays in Core
- [ ] MCP tool execution audit record includes `actingContext` (Story 17.8)
- [ ] If `CredentialResolver` returns `null` for a required credential:
  - Tool returns `{ error: 'credential_unavailable', service, hint: 'Request delegation via delegation-request API' }`
  - Agent runtime can interpret this as a signal to trigger an interactive delegation request (Story 17.4)
- [ ] `X-Sera-Acting-Context` header validated on receipt by the MCP proxy handler — malformed or tampered context rejected, tool call aborted

**Technical Notes:**
- The MCP server itself can choose to use or ignore the acting context — it is advisory for servers that support per-user credential selection. For servers that use a fixed service account, the injected credential is always the agent's own service identity regardless of the acting context.
- This design keeps MCP servers stateless with respect to auth context — they receive what they need per-call rather than maintaining session state

---

## Cross-epic impacts

The following existing stories require updates once this epic is implemented:

| Story | Change required |
|-------|----------------|
| **16.7 (SecretsProvider interface)** | `SecretAccessContext` gains `actingContext?: ActingContext` — providers can use it for fine-grained access decisions |
| **16.9 (Secret injection at spawn)** | Clarify: spawn-time injection is for agent-autonomous secrets only; delegated credentials are never injected at spawn |
| **3.9 (Permission request service)** | Shared UX pattern — the `system.delegation-requests` Centrifugo channel and decision endpoint in this epic follow the same operator interaction model |
| **7.3 (Containerised MCP servers)** | MCP container env injection distinguishes spawn-time (autonomous) vs per-call (delegated) credentials |
| **11.x (Audit trail)** | Audit trail schema gains `acting_context` JSONB column; Merkle hash includes this field |
| **13.x (sera-web agent UX)** | Delegation request dialog (parallel to permission request dialog) added to agent detail view |
| **14.x (sera-web observability)** | Delegation management panel: active delegations per agent, revoke action, delegation audit log filter |

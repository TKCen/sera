# SPEC: Identity & Authorization (`sera-auth`)

> **Status:** DRAFT
> **Source:** PRD §8 (all subsections), §11.1, §11.3, §14 (invariants 5, 6, 13), plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §7 (crate-by-crate auth stack choices), [SPEC-self-evolution](SPEC-self-evolution.md) §5.2, §6, §7, §12 (`MetaChange`/`CodeChange`/`MetaApprover` capabilities, capability tokens with narrowing, constitutional anchor, meta-change protection)
> **Crate:** `sera-auth`
> **Priority:** Phase 1 (basic), Phase 4 (enterprise)

---

## 1. Overview

SERA uses a **Principal-centric identity model**. Any acting entity — human, agent, service, or external agent — is a **Principal**. Principals have identity, credentials, and authorization. Agents are **first-class citizens** in the identity system: they authenticate, they are authorized (or denied), and they appear in audit logs just like humans.

The identity and authorization system has three layers:
1. **Identity Layer** — who is this principal? (AuthN)
2. **Authorization Layer** — what can this principal do? (AuthZ)
3. **Continuous Security Posture** — is this principal's access still valid? (SSF/CAEP/RISC)

---

## 2. Principal Model

```rust
pub enum Principal {
    Human(HumanPrincipal),
    Agent(AgentPrincipal),
    ExternalAgent(ExternalAgentPrincipal),
    Service(ServicePrincipal),
}

pub struct HumanPrincipal {
    pub id: PrincipalId,
    pub name: String,
    pub email: Option<String>,
    pub groups: Vec<PrincipalGroupId>,
    pub auth_method: AuthMethod,       // Local, OIDC, SCIM-provisioned
}

pub struct AgentPrincipal {
    pub id: PrincipalId,
    pub agent_id: AgentId,
    pub name: String,
    pub groups: Vec<PrincipalGroupId>,
    pub credentials: AgentCredentials,  // API keys, tokens for tool access
    pub risk_profile: RiskProfile,
}

pub struct ExternalAgentPrincipal {
    pub id: PrincipalId,
    pub protocol: ExternalProtocol,     // A2A (ACP merged into A2A — see SPEC-interop §5)
    pub external_id: String,
    pub trust_level: TrustLevel,
    pub registered_by: PrincipalRef,
}

pub struct ServicePrincipal {
    pub id: PrincipalId,
    pub name: String,
    pub service_type: String,           // CI/CD, monitoring, etc.
    pub api_key: ApiKeyRef,
}
```

---

## 3. Principal Groups

Principals are grouped into **PrincipalGroups** for RBAC, authorization, and approval routing. Groups can contain **both humans and agents**.

```rust
pub struct PrincipalGroup {
    pub id: PrincipalGroupId,
    pub name: String,
    pub members: Vec<PrincipalRef>,
    pub roles: Vec<RoleName>,
}
```

---

## 4. Identity Layer (AuthN)

### 4.1 Auth Methods

| Method | Tier | Crate (from SPEC-dependencies §7) | Description |
|---|---|---|---|
| API Keys | 1, 2, 3 | hand-rolled bearer middleware over sqlx KV | Simple token-based auth for services and agents |
| JWT | 1, 2, 3 | [`jsonwebtoken` ^10.3](https://crates.io/crates/jsonwebtoken) | Stateless token auth for humans and services; validates `exp`, `nbf`, `aud`, `iss` automatically |
| Basic Auth | 1 | `argon2` (RustCrypto) for password hashing | Simple username/password (local dev only) |
| OAuth2 | 2, 3 | [`oauth2` ^5.0](https://crates.io/crates/oauth2) | Delegated authorization; typestate-tracked endpoints |
| OIDC | 3 | [`openidconnect` ^3.5](https://crates.io/crates/openidconnect) | Federated identity (enterprise SSO); requires background JWKS refresh task (write-yourself glue) |
| SCIM | 3 | [`scim-server` ^0.5](https://crates.io/crates/scim-server) + [`scim_v2`](https://crates.io/crates/scim_v2) (scaffolding — SERA writes PATCH filter ops + Principal mapping) | Identity provisioning from enterprise directories |
| Session cookies | 2, 3 | [`axum-login`](https://crates.io/crates/axum-login) + [`tower-sessions`](https://crates.io/crates/tower-sessions) | Browser session management; backed by Redis or PostgreSQL |
| (shortcut) | 2, 3 | [`ory-kratos-client`](https://crates.io/crates/ory-kratos-client) — out-of-process sidecar | Fastest path to complete human OIDC + MFA + social login without writing the whole enterprise stack — SERA delegates to Kratos and verifies sessions via its Admin API |

**Feature gating.** Enterprise auth methods are gated behind the `enterprise` Cargo feature per [SPEC-crate-decomposition](SPEC-crate-decomposition.md) §6.2. Tier-1 deployments compile with `default = ["jwt", "basic-auth"]` and do not pay the dependency cost of OIDC/SCIM.

### 4.2 Agent Authentication

Agents authenticate as principals with their own credentials. These credentials are:
- Managed by the system (auto-generated API keys)
- Rotatable without agent downtime
- Scoped to the agent's authorization boundaries

### 4.3 External Agent Identity Registration

External agents (from A2A, MCP) are registered as `ExternalAgentPrincipal` entries. Registration is performed by an authorized principal (admin or operator) and includes trust level assignment. ACP agents were previously a separate path but the protocol merged into A2A on 2025-08-25 (see [SPEC-interop](SPEC-interop.md) §5) — they now register as A2A external agents via the `acp-compat` Cargo feature translator.

### 4.4 Auth Stack

### 4.2 Agent Authentication

Agents authenticate as principals with their own credentials. These credentials are:
- Managed by the system (auto-generated API keys)
- Rotatable without agent downtime
- Scoped to the agent's authorization boundaries

### 4.3 External Agent Identity Registration

External agents (from A2A, ACP, MCP) are registered as `ExternalAgentPrincipal` entries. Registration is performed by an authorized principal (admin or operator) and includes trust level assignment.

### 4.4 Auth Stack

```
┌──────────────────────────────────────────────────────┐
│  Identity Layer (all principals)                      │
│  Local: JWT + API Keys + Basic Auth                   │
│  Enterprise: OAuthv2, OIDC, SCIM provisioning         │
│  Agents: Registered principal with own credentials    │
│  External: A2A/ACP agent identity registration        │
├──────────────────────────────────────────────────────┤
│  Authorization Layer                                  │
│  Local: Built-in RBAC (roles + permissions)           │
│  Enterprise: AuthZen PDP integration                  │
│  + Hook-contributed authz checks at any point         │
│  + Dynamic risk-based assessment                      │
├──────────────────────────────────────────────────────┤
│  Continuous Security Posture                          │
│  Enterprise: Shared Signals Framework (SSF)           │
│  CAEP events (session revocation, compliance)         │
│  RISC events (credential compromise, user risk)       │
└──────────────────────────────────────────────────────┘
```

---

## 5. Authorization Layer (AuthZ)

### 5.1 AuthZ Trait (Pluggable PDP)

```rust
#[async_trait]
pub trait AuthorizationProvider: Send + Sync {
    async fn check(
        &self,
        principal: &Principal,
        action: &Action,
        resource: &Resource,
        context: &AuthzContext,
    ) -> Result<AuthzDecision, AuthzError>;
}

pub enum AuthzDecision {
    Allow,
    Deny(DenyReason),
    NeedsApproval(ApprovalSpec),       // Escalate to HITL
}

pub enum Action {
    Read,
    Write,
    Execute,
    Admin,
    ToolCall(String),                  // Specific tool name
    ProposeChange(BlastRadius),         // Self-evolution: propose a Change Artifact (SPEC-self-evolution §9)
    ApproveChange(ChangeArtifactId),    // Self-evolution: sign off on a meta-change
}

pub enum Resource {
    Session(SessionRef),
    Agent(AgentRef),
    Tool(ToolRef),
    Memory(MemoryScope),
    Config(ConfigPath),
    Workflow(WorkflowRef),
    ChangeArtifact(ChangeArtifactId),   // Self-evolution artifact — Tier 2/3
}
```

### 5.1a Capability Model (SERA-specific)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §5.2.

Beyond the standard `Action`/`Resource` pair, SERA defines a set of **structured capabilities** carried on every Principal's identity. Capabilities are strictly bounded — they cannot be requested at runtime, only granted at Principal creation or via a Tier-2 config change.

```rust
pub enum AgentCapability {
    // Basic capabilities
    Read,
    Write,
    Execute,
    SessionJoin,

    // Delegation
    Delegation,                          // Injects DelegateWorkTool into the agent's tool registry

    // Config
    ConfigRead,
    ConfigPropose,                        // Propose ordinary Tier-2 config changes
    DocsRead,

    // Self-evolution (SPEC-self-evolution §5.2, §7)
    MetaChange,                           // Can propose Tier-2 Change Artifacts (config self-evolution)
    CodeChange,                           // Can propose Tier-3 Change Artifacts (code self-evolution) — distinct from MetaChange
    MetaApprover,                         // Can sign off on meta-changes (approval-path capability, must exist outside the agent graph per CON-01)
}
```

**Principal standing.** The `MetaChange` and `CodeChange` capabilities are held only by a narrow set of administrative agents. `MetaApprover` is held by principals (typically human operators or dedicated reviewer agents) that are specifically pinned in the constitutional anchor — `CON-01` requires at least one `MetaApprover` outside the agent graph at all times.

### 5.1b Capability Tokens with Narrowing

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §12.

For self-evolution work and for agents acting under delegated authority, SERA issues **capability tokens** — time-limited, scope-narrowed credentials that cannot grant more than the issuing principal already holds.

```rust
pub struct CapabilityToken {
    pub id: CapabilityTokenId,
    pub holder: PrincipalRef,
    pub issuer: PrincipalRef,                     // Who granted it
    pub scopes: HashSet<BlastRadius>,             // From SPEC-self-evolution §9
    pub max_proposals: u32,                        // Consumed per Change Artifact
    pub proposals_consumed: u32,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revocation_check_required: bool,           // All Tier-3 tokens = true
    pub signature: TokenSignature,
}
```

**Narrowing rule (enforced at issuance):** a token's `scopes` set MUST be a subset of the issuer's own capability set. An admin holding `{SingleHookConfig, SingleToolPolicy}` cannot issue a token with `{GatewayCore}`. This closes the "admin grants more than it has" attack class.

**Use-per-proposal counting.** Every Change Artifact submission consumes one `proposals_consumed` increment. Exhausted tokens are rejected at the next submission. This forces rate limiting at the token layer rather than the application layer.

**Revocation.** A revocation list is stored in the audit log. Tokens with `revocation_check_required: true` are checked against the list at every use (every Tier-3 token qualifies).

### 5.2 Authorization Check Points

The AuthZ trait is called at **every authorization-relevant point**:

| Check Point | Question |
|---|---|
| Gateway event ingress | Can this principal interact with this agent? |
| Tool execution | Can this principal/agent run this tool in this context? |
| Memory access | Can this principal read/write this memory scope? |
| Config changes | Can this principal modify this agent's config? |
| Session operations | Can this principal view/join this session? |
| Workflow triggers | Can this principal trigger this workflow? |
| **Change Artifact proposal** | Does this principal hold the capability token for this blast radius? (SPEC-self-evolution §9) |
| **Meta-change approval** | Is this principal in the pinned `MetaApprover` set for this meta-change? (SPEC-self-evolution §7) |

### 5.3 Built-in RBAC

The default `AuthorizationProvider` implementation uses [`casbin` 2.19](https://crates.io/crates/casbin). Policy model is domain-tenanted RBAC via Casbin's PERM model config. Avoid `oso` — it has pivoted toward a SaaS model and the embedded crate has reduced momentum.

| Role | Scope |
|---|---|
| **Admin** | Full system config, agent lifecycle, principal management |
| **Operator** | Agent config, session management, monitoring |
| **User** | Interact with assigned agents within authorized scopes |
| **Observer** | Read-only access to transcripts and metrics |
| **Agent** | Default role for agent principals — scoped by tool profiles and policies |
| **MetaApprover** | Signs meta-changes per §5.1a — typically mapped to operators or dedicated reviewer agents outside the live event graph |

### 5.2 Authorization Check Points

The AuthZ trait is called at **every authorization-relevant point**:

| Check Point | Question |
|---|---|
| Gateway event ingress | Can this principal interact with this agent? |
| Tool execution | Can this principal/agent run this tool in this context? |
| Memory access | Can this principal read/write this memory scope? |
| Config changes | Can this principal modify this agent's config? |
| Session operations | Can this principal view/join this session? |
| Workflow triggers | Can this principal trigger this workflow? |

### 5.3 Built-in RBAC

The default `AuthorizationProvider` implementation is role-based access control:

| Role | Scope |
|---|---|
| **Admin** | Full system config, agent lifecycle, principal management |
| **Operator** | Agent config, session management, monitoring |
| **User** | Interact with assigned agents within authorized scopes |
| **Observer** | Read-only access to transcripts and metrics |
| **Agent** | Default role for agent principals — scoped by tool profiles and policies |

### 5.4 Enterprise: AuthZen PDP

For enterprise deployments, the `AuthorizationProvider` trait can delegate to an external AuthZen-compliant Policy Decision Point. This enables centralized policy management across the organization.

**No Rust crate exists** for AuthZen clients as of this spec. SERA writes a ~60-line `reqwest` wrapper over the canonical `POST /access/v1/evaluation` endpoint per [SPEC-dependencies](SPEC-dependencies.md) §7. The request shape is:

```json
{
  "subject":  { "type": "user",  "id": "<principal-id>" },
  "resource": { "type": "agent", "id": "<resource-id>" },
  "action":   { "name": "execute" },
  "context":  {}
}
```

Response is `{ "decision": true|false }`. The AuthZen Authorization API reached Final Specification status in 2025.

### 5.5 Hook-Contributed AuthZ

WASM hooks can contribute **additional authz checks** at any hook point. Hooks receive the authz context and can return `Reject` with a reason. This allows fine-grained, context-dependent authorization that goes beyond role-based checks.

---

## 6. Continuous Security Posture (Enterprise)

### 6.1 Shared Signals Framework (SSF)

Enterprise deployments can integrate with SSF for continuous access evaluation:

- **CAEP** (Continuous Access Evaluation Protocol) — session revocation, compliance-driven access changes
- **RISC** (Risk Incident Sharing and Coordination) — credential compromise alerts, user risk score changes

When SSF events are received, the gateway can:
- Revoke active sessions for affected principals
- Downgrade trust levels for external agents
- Trigger re-authentication
- Escalate approval requirements

**No Rust crate exists** for SSF/CAEP/RISC as of this spec — the three specs reached final status in September 2025 and have zero Rust implementations. SERA writes a SET (Security Event Token) ingester on top of `jsonwebtoken` per [SPEC-dependencies](SPEC-dependencies.md) §7. The work is approximately:

1. Receive a signed SET over HTTP push or polling
2. Verify the SET signature via `jsonwebtoken` (SETs are JWTs)
3. Parse the `events` claim and dispatch to a revocation handler
4. Optionally trigger a self-evolution `RollbackRequest` Change Artifact for severe RISC events

Estimated at ~300 lines of Rust beyond what `jsonwebtoken` already provides.

---

## 7. Configuration

```yaml
sera:
  auth:
    # Identity
    method: "jwt"                       # jwt | basic | oidc
    jwt:
      secret: { secret: "auth/jwt/secret" }
      issuer: "sera"
      expiry: "24h"

    # Authorization
    authz:
      provider: "built-in"              # built-in | authzen
      authzen:
        pdp_url: "https://pdp.example.com"

    # Enterprise
    oidc:
      issuer: "https://idp.example.com"
      client_id: { secret: "auth/oidc/client-id" }
      client_secret: { secret: "auth/oidc/client-secret" }

    scim:
      enabled: false
      endpoint: "/scim/v2"

    ssf:
      enabled: false
      caep_endpoint: "https://ssf.example.com/caep"
      risc_endpoint: "https://ssf.example.com/risc"
```

---

## 8. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 5 | Capability ≠ execution | Tool schemas exposed ≠ execution authorized |
| 6 | Session key = routing ≠ authorization | Session key drives routing; authz checked separately |
| 13 | Principal identity is traceable | All acting entities are principals in audit logs |

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | AuthN/AuthZ enforcement at ingress; `MetaChange`/`CodeChange` capability consumption on `Op::UserTurn` |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool execution authorization |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | `NeedsApproval` decision escalation; `MetaApprover` pinning for meta-changes |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook-contributed authz checks; `constitutional_gate` consumes capability tokens |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Credential storage for agent principals |
| `sera-interop` | [SPEC-interop](SPEC-interop.md) | External agent identity registration (A2A only; ACP merged into A2A) |
| `sera-circles` | [SPEC-circles](SPEC-circles.md) | Relationship with PrincipalGroups (see open questions); `operator_approver_set` pinning |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | `MetaChange`/`CodeChange`/`MetaApprover` capabilities (§5.1a); capability tokens with narrowing (§5.1b); constitutional rule set verifies signer fingerprint |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §7 crate-by-crate stack: `jsonwebtoken` / `oauth2` / `openidconnect` / `casbin` / `argon2` / `axum-login` / `scim-server` / `ory-kratos-client` + write-yourself AuthZen + SSF/CAEP/RISC |

---

## 10. Open Questions

1. **Circles vs. PrincipalGroups overlap** — When a Circle also needs authorization boundaries, should Circles automatically create PrincipalGroups, or remain purely coordination? (Medium priority — Phase 4)
2. **Agent credential rotation** — What's the rotation mechanism for agent API keys? Automatic? Manual? Grace period?
3. **Trust level granularity** — What are the concrete trust levels for ExternalAgentPrincipal? How do they map to authorization scopes?
4. **Anonymous/unauthenticated access** — Is there a path for unauthenticated access in Tier 1 (local dev)? Or does Tier 1 just auto-create a default admin principal?
5. **Multi-tenancy** — The Organization is the top-level tenant. How is multi-org support handled? Single gateway per org? Or multi-org gateway?

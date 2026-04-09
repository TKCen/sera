# SPEC: Identity & Authorization (`sera-auth`)

> **Status:** DRAFT  
> **Source:** PRD §8 (all subsections), §11.1, §11.3, §14 (invariants 5, 6, 13)  
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
    pub protocol: ExternalProtocol,     // A2A, ACP
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

| Method | Tier | Description |
|---|---|---|
| API Keys | 1, 2, 3 | Simple token-based auth for services and agents |
| JWT | 1, 2, 3 | Stateless token auth for humans and services |
| Basic Auth | 1 | Simple username/password (local dev only) |
| OAuthv2 | 2, 3 | Delegated authorization |
| OIDC | 3 | Federated identity (enterprise SSO) |
| SCIM | 3 | Identity provisioning from enterprise directories |

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
}

pub enum Resource {
    Session(SessionRef),
    Agent(AgentRef),
    Tool(ToolRef),
    Memory(MemoryScope),
    Config(ConfigPath),
    Workflow(WorkflowRef),
}
```

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
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | AuthN/AuthZ enforcement at ingress |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Tool execution authorization |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | `NeedsApproval` decision escalation |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Hook-contributed authz checks |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Credential storage for agent principals |
| `sera-interop` | [SPEC-interop](SPEC-interop.md) | External agent identity registration |
| `sera-circles` | [SPEC-circles](SPEC-circles.md) | Relationship with PrincipalGroups (see open questions) |

---

## 10. Open Questions

1. **Circles vs. PrincipalGroups overlap** — When a Circle also needs authorization boundaries, should Circles automatically create PrincipalGroups, or remain purely coordination? (Medium priority — Phase 4)
2. **Agent credential rotation** — What's the rotation mechanism for agent API keys? Automatic? Manual? Grace period?
3. **Trust level granularity** — What are the concrete trust levels for ExternalAgentPrincipal? How do they map to authorization scopes?
4. **Anonymous/unauthenticated access** — Is there a path for unauthenticated access in Tier 1 (local dev)? Or does Tier 1 just auto-create a default admin principal?
5. **Multi-tenancy** — The Organization is the top-level tenant. How is multi-org support handled? Single gateway per org? Or multi-org gateway?

# SPEC: Security Model

> **Status:** DRAFT  
> **Source:** PRD §16, with cross-cutting concerns from §5, §8, §10.3, §13, §14  
> **Priority:** Phase 0 (foundational), ongoing  

---

## 1. Overview

SERA's security model is based on **defense in depth** with distinct trust boundaries. The system is designed so that security and isolation are enforced by the architecture, not by prompts or conventions.

---

## 2. Trust Boundaries

```
┌─────────────────────────────────────────────────────────┐
│  TRUSTED CORE (sera-gateway process)                     │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────┐  │
│  │ sera-auth     │ │ sera-session │ │ sera-runtime   │  │
│  │ (Principal    │ │ (state mach) │ │ (ctx pipeline) │  │
│  │  Registry +   │ │              │ │                │  │
│  │  AuthZ PDP)   │ │              │ │                │  │
│  └──────────────┘ └──────────────┘ └────────────────┘  │
│  ┌──────────────┐ ┌──────────────────────────────────┐  │
│  │ sera-secrets  │ │ WASM Sandbox (sera-hooks)        │  │
│  │ (secret mgr)  │ │  - fuel metered, memory capped   │  │
│  └──────────────┘ │  - no host FS/net unless granted  │  │
│                    │  - config-driven parameterization │  │
│                    └──────────────────────────────────┘  │
├─────────────────────── gRPC boundary ───────────────────┤
│  ISOLATED ADAPTERS (crash independently)                 │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────┐  │
│  │ Connectors   │ │ Ext Tools    │ │ Ext Runtimes   │  │
│  └──────────────┘ └──────────────┘ └────────────────┘  │
├─────────────────────── Client boundary ─────────────────┤
│  UNTRUSTED CLIENTS (all principals)                      │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐         │
│  │ CLI  │ │ TUI  │ │ Web  │ │ SDK  │ │ HMI  │         │
│  └──────┘ └──────┘ └──────┘ └──────┘ └──────┘         │
└─────────────────────────────────────────────────────────┘
```

### 2.1 Trusted Core

The gateway process is the **trusted core**. It contains:
- **Identity and authorization** (`sera-auth`) — all principal verification
- **Secret management** (`sera-secrets`) — secret resolution, never exposed
- **Session state** (`sera-session`) — session integrity
- **Runtime** (`sera-runtime`) — context assembly, model calls
- **WASM sandbox** (`sera-hooks`) — hooks run in sandboxed WASM, NOT with host privileges

### 2.2 WASM Sandbox

Hooks run inside the WASM sandbox with strict constraints:
- **Fuel metering** — computation budget per invocation
- **Memory caps** — memory ceiling per hook instance
- **No ambient host access** — no filesystem, no network unless explicitly granted via WASM component model capabilities
- **Deterministic termination** — fuel + timeout guarantee hooks always terminate

### 2.3 gRPC Boundary

External adapters (connectors, tools, runtimes, model providers, secret providers) run in **separate processes** and communicate via gRPC. They:
- **Crash independently** — an adapter crash does not bring down the gateway
- **Have limited trust** — the gateway validates all adapter responses
- **Authenticate** — adapters authenticate to the gateway
- **Are isolated** — no direct memory sharing with the trusted core

### 2.4 Client Boundary

All clients are **untrusted**. Every client request is:
- Authenticated (principal resolution)
- Authorized (AuthZ check)
- Rate limited (optionally, via hooks)
- Deduplicated (idempotency keys)
- Audited (audit log)

---

## 3. Security Principles

| Principle | Implementation |
|---|---|
| **Zero trust clients** | All client input is validated and authorized |
| **Capability ≠ execution** | Exposing a tool schema does NOT grant execution permission |
| **Least privilege** | Principals (including agents) get minimum necessary permissions |
| **Defense in depth** | Multiple enforcement layers: authz, hooks, tool policy, approval gates |
| **Secrets isolation** | Secrets never in config, logs, or API responses |
| **Sandboxed extensibility** | Hooks run in WASM sandbox; adapters behind gRPC |
| **Traceable identity** | Every action attributable to a principal |
| **Evidence preservation** | All security-relevant actions produce audit records |

---

## 4. Attack Surface Mitigation

### 4.1 Prompt Injection

SERA does **not** rely on prompts for security. Security invariants (tool policies, authz checks, approval gates) are enforced by code, not by system prompt instructions.

- Tool execution requires AuthZ check regardless of what the model requests
- Approval gates cannot be bypassed by model output
- Memory writes go through hook chains regardless of content

### 4.2 Secret Exfiltration

- Secrets are resolved at runtime by the secret provider, not stored in config
- Secret values are never logged
- Hook chains can filter outgoing responses for secret-like patterns (PII redactor pattern)
- Tool results go through `post_tool` hooks before reaching the model

### 4.3 Resource Exhaustion

- WASM hooks have fuel limits and memory caps
- Queue has global concurrency throttle
- Tool calls have iteration limits
- Timeouts at every async boundary

### 4.4 Adapter Compromise

- Adapters run in separate processes (gRPC boundary)
- Adapter responses are validated by the gateway
- A compromised adapter cannot access other sessions, agents, or secrets

### 4.5 Unauthorized Escalation

- All tool calls go through AuthZ check
- Risk-based approval routing for sensitive actions
- Agents have their own principal identity with bounded permissions
- External agent identities have explicit trust levels

### 4.6 PII / Sensitive Data Tokenization

> **Enhancement: Anthropic Code Execution with MCP**

When tool results contain sensitive data (PII, credentials, internal identifiers), a `post_tool` hook can **tokenize** sensitive values before they reach the model context:

```
Tool returns: "User email: john@example.com, SSN: 123-45-6789"
Post-tool hook transforms to: "User email: [EMAIL_1], SSN: [SSN_1]"
```

The tokenization map is maintained in the turn context:
- Tokenized references can be **detokenized** when passed to subsequent tool calls (e.g., sending an email to `[EMAIL_1]`)
- Token maps are **never logged** and **never persisted** to memory
- Token maps are **scoped to the turn** — they do not persist across turns

**Implementation:** This is a `post_tool` hook pattern, not a core system feature. SERA should ship a reference PII tokenization hook module:

```yaml
hooks:
  post_tool:
    - name: "pii-tokenizer"
      module: "sera-hooks-pii"          # Built-in reference hook
      config:
        patterns:
          - type: "email"
            regex: "[a-zA-Z0-9.]+@[a-zA-Z0-9.]+"
            token_prefix: "EMAIL"
          - type: "ssn"
            regex: "\\d{3}-\\d{2}-\\d{4}"
            token_prefix: "SSN"
        detokenize_on_tool_input: true   # Detokenize when passing to tools
```

> [!IMPORTANT]
> PII tokenization is defense-in-depth. It does not replace proper access controls — the authorization system should prevent agents from accessing sensitive data in the first place. Tokenization protects against *incidental* exposure when tools legitimately return data containing embedded PII.

---

## 5. Data Protection

### 5.1 Data at Rest

| Data Type | Protection |
|---|---|
| Database (SQLite/PostgreSQL) | Filesystem permissions; optional encryption at rest |
| Memory files | Filesystem permissions; optional git encryption |
| Secrets (file provider) | Encrypted (e.g., age) |
| Audit logs | Append-only, database-backed |

### 5.2 Data in Transit

| Channel | Protection |
|---|---|
| Client → Gateway | TLS (configurable; optional for local dev) |
| Gateway → Adapter (gRPC) | TLS or Unix socket (local) |
| Gateway → Model Provider | TLS (depends on provider config) |
| Gateway → Secret Provider | TLS |

### 5.3 Data in Use

- Secret values are held in memory only during resolution; not persisted in logs or responses
- Context assembly does not leak sensitive data from one session to another
- Hook sandboxing prevents hooks from accessing data outside their HookContext

---

## 6. Relevant Invariants

| # | Invariant | Security Relevance |
|---|---|---|
| 5 | Capability ≠ execution | Prevents unauthorized tool execution |
| 7 | Memory writes are privileged | Prevents unauthorized memory modification |
| 8 | Hooks are sandboxed | Prevents hook code from compromising the system |
| 9 | Adapters are isolated | Prevents adapter compromise from spreading |
| 13 | Principal identity is traceable | Enables incident investigation |
| 14 | Secrets never in config | Prevents secret leakage via config exposure |

---

## 7. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN/AuthZ enforcement |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | WASM sandboxing |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret isolation |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval gates for sensitive actions |
| `sera-observability` | [SPEC-observability](SPEC-observability.md) | Audit logging |
| Deployment | [SPEC-deployment](SPEC-deployment.md) | Trust boundaries per tier |

---

## 8. Open Questions

1. **TLS configuration** — What's the TLS story for Tier 1? Self-signed certs? TLS optional?
2. **Content Security Policy** — Should the web client enforce CSP headers? XSS mitigation?
3. **Rate limiting architecture** — Is rate limiting built into the gateway, or purely hook-driven?
4. **Audit log retention** — What's the default retention policy? Is it configurable?
5. **Vulnerability disclosure** — What's the security vulnerability reporting process?
6. **Security auditing** — Is there a plan for third-party security audits?

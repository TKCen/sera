# SPEC: Security Model

> **Status:** DRAFT
> **Source:** PRD §16, with cross-cutting concerns from §5, §8, §10.3, §13, §14, plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §10.8 (NemoClaw **Sentry-class multi-tenant SaaS exfiltration threat class**, deny-by-default filesystem policy, Landlock rule-union gotcha, pinned-image dual-field lockstep), §10.18 (NVIDIA OpenShell **`allowed_ips: [CIDR]`** SSRF mitigation, **per-binary SHA-256 trust-on-first-use**, in-process OPA via `regorus`, hot-reload policy with version tracking + SHA-256 integrity), [SPEC-self-evolution](SPEC-self-evolution.md) §6 (constitutional anchor), §13 (kill switch), §14 (12 deadlock-prevention patterns for trust collapse, audit immutability, approval self-loop)
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
- **Sandbox credentials never touch the sandbox filesystem** — they are injected as env vars over gRPC at sandbox startup (SPEC-secrets §5a)
- **`inference.local` virtual host** — the agent never sees the real provider API key; the proxy injects it on outbound (SPEC-secrets §5b)

### 4.2a Multi-Tenant SaaS Exfiltration Channel (NEW threat class)

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.8 NemoClaw issue #1437 — the Sentry.io case study.

**Threat:** an endpoint that looks like a legitimate dependency (telemetry, analytics, error reporting) is actually a **multi-tenant SaaS** where any authenticated client can post to any account. If SERA grants `POST *` to such a host, a compromised agent can exfiltrate arbitrary data by posting to an attacker-controlled tenant on the same host.

**Concrete example from NemoClaw's production policy:**

> sentry.io is a multi-tenant SaaS — any authenticated client can POST to ANY Sentry project, not just the caller's. Allowing POST /** turned the host into a generic exfiltration channel: a compromised agent could ship stack traces, env vars, file contents, etc. to a Sentry project controlled by an attacker via the public envelope endpoint (https://sentry.io/api/<any-project>/envelope/). Path-pattern restrictions cannot fix this because the project ID is part of the URL and there is no server-side allowlist of legitimate projects. Block POST entirely. GET stays allowed because it has no request body and is harmless for exfil. Side effect: Claude Code's crash telemetry to Sentry is silently dropped — that is the right tradeoff for a sandbox whose stated goal is preventing data egress.

**SERA's rule:** any endpoint whose tenant boundary is not per-user path-isolated defaults to **GET-only**. Examples:

- `sentry.io` — multi-tenant; allow `GET /**` only
- `api.github.com` (write endpoints) — scoped by token, but POST-to-a-repo-I-don't-own is still a concern with a compromised token; limit POST to specific repos
- `discord.com` (webhook endpoints) — any webhook ID is accepted; if the agent has one legitimate webhook, it also has a generic exfil channel; limit by URL-prefix match
- Cloud provider telemetry endpoints — treat every "drop a blob somewhere" endpoint as an exfiltration risk

The tradeoff (lost legitimate telemetry to Sentry) is the right one for a sandbox whose stated goal is preventing data egress. **Operators who need telemetry must set up their own dedicated telemetry endpoint on a single-tenant host.**

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
- **Per-binary SHA-256 trust-on-first-use** — network egress rules bind to the caller binary's content hash; a substituted binary fails the TOFU check (SPEC-tools §6a.2)
- **`allowed_ips: [CIDR]` SSRF mitigation** — beyond host-pattern filtering, network policies include CIDR allowlists (SPEC-tools §6a.0, SPEC-dependencies §10.18)
- **Capability tokens with narrowing** — delegated authority cannot exceed the grantor's own capabilities (SPEC-identity-authz §5.1b)

### 4.6 Trust Collapse Prevention (Self-Evolution)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §14.6.

**Threat:** a compromised admin uses legitimate privileges to remove audit/rollback infrastructure, then deletes evidence of its actions.

**Mitigations:**

1. **Separate audit write path** — the audit log backend is bound at boot via `OnceCell` and is NOT reachable from any Change Artifact code path. Compromised admin code cannot forge audit entries because the normal event pipeline credentials don't authorize writes to the audit backend (SPEC-observability §3.2).
2. **Cryptographic chain** — audit entries form a content-hash chain. Tampering breaks the chain; gateway refuses to boot on broken chain.
3. **`CON-03`** — rollback pointers cannot be deleted within their rollback window. Enforced by the constitutional gate before any meta-change touches them.
4. **Meta-quorum with pinned approvers** — removing a `MetaApprover` from the approver set requires signatures from the **frozen** approver set at the time the change was proposed, not the live set. A compromised admin cannot revoke the approvers mid-flight.
5. **Operator offline key** — changes to the audit log backend, constitutional rule set, kill switch protocol, or self-evolution pipeline require a signature from a key held outside the running SERA instance (HSM, air-gapped device, Shamir shares). See SPEC-deployment §3.2.

### 4.7 Approval Self-Loop Prevention

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §14.3, §7.

**Threat:** a meta-change proposes to remove the very approver that gates its own approval.

**Mitigation:** approver pinning. When a meta-change is proposed, the current `MetaApprover` principal set is **frozen**. The meta-change is evaluated against the frozen set, not the live set. This makes the "remove the approver then push the change" attack structurally impossible — the frozen set always contains the approvers the change wants to remove, so they get to vote on their own removal. If they decline, the change is rejected; if they approve, they consented to their removal explicitly.

### 4.8 Hook Short-Circuit Prevention

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §14.4.

**Threat:** a new `pre_route` hook denies the rollback traffic that would have removed it.

**Mitigation:** the kill switch admin socket bypasses the entire hook pipeline. Rollback through the kill switch (SPEC-gateway §7a.4, SPEC-self-evolution §13) does not flow through any user-configurable hook chain — it goes directly to the constitutional gate's compiled-in rollback path.

### 4.9 Tool-Loss Deadlock Prevention

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §14.5, CON-08.

**Threat:** an agent disables the tool it would use to re-enable the tool.

**Mitigation:** `CON-08` — a change cannot remove the capability required to roll it back. Dependency analysis at dry-run time detects whether the proposed change removes any tool/capability/path required by its rollback artifact. If yes, the proposal is rejected before it ever reaches the approval gate. This closes the class of self-locking changes entirely.

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
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | AuthN/AuthZ enforcement; capability tokens with narrowing; `MetaApprover` role |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | WASM sandboxing; constitutional gate (fail-closed, compiled-in) |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret isolation; gRPC sandbox credential injection; `inference.local` virtual host |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval gates for sensitive actions; pinned meta-approvers |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Three-layer sandbox policy; CIDR allowlist; binary TOFU; deny-by-default filesystem; multi-tenant SaaS exfil class |
| `sera-observability` | [SPEC-observability](SPEC-observability.md) | Audit logging; OCSF v1.7.0 taxonomy; separate write path with cryptographic chain |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Constitutional anchor (§6); kill switch (§13); 12 deadlock-prevention patterns (§14); operator offline key requirements |
| Deployment | [SPEC-deployment](SPEC-deployment.md) | Trust boundaries per tier; operator offline key storage |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §10.8 NemoClaw Sentry exfil case + Landlock gotcha + pinned image lockstep; §10.18 OpenShell CIDR allowlist + binary TOFU + `regorus` in-process OPA + hot-reload with SHA-256 integrity |

---

## 8. Open Questions

1. **TLS configuration** — What's the TLS story for Tier 1? Self-signed certs? TLS optional?
2. **Content Security Policy** — Should the web client enforce CSP headers? XSS mitigation?
3. **Rate limiting architecture** — Is rate limiting built into the gateway, or purely hook-driven?
4. **Audit log retention** — What's the default retention policy? Is it configurable?
5. **Vulnerability disclosure** — What's the security vulnerability reporting process?
6. **Security auditing** — Is there a plan for third-party security audits?

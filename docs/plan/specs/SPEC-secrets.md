# SPEC: Secret Management (`sera-secrets`)

> **Status:** DRAFT  
> **Source:** PRD §10.3, §13 (SecretProviderService proto)  
> **Crate:** `sera-secrets`  
> **Priority:** Phase 0 (env + file), Phase 4 (enterprise providers)  

---

## 1. Overview

Secrets are **never stored in config files**. SERA provides a pluggable secret management system that supports simple providers (environment variables, files) for development and external secret managers (Vault, AWS SM, Azure KV, GCP SM) for enterprise deployments.

Secrets management in SERA goes beyond simple key-value resolution. It includes:
- **Pluggable providers** with a unified trait
- **Credential injection** into tool contexts via hooks
- **Agent-initiated secret entry** — "side-routed enterability" flow
- **Authorization hooks** for secret access control
- **Reference-only config syntax** — secrets in config files are references, not values

---

## 2. Secret Provider Trait

```rust
#[async_trait]
pub trait SecretProvider: Send + Sync {
    /// Resolve a secret reference to its value
    async fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, SecretError>;

    /// List available secret paths (for introspection, not values)
    async fn list_paths(&self, prefix: &str) -> Result<Vec<String>, SecretError>;

    /// Store a new secret (for providers that support write)
    async fn store(&self, path: &str, value: SecretValue) -> Result<(), SecretError>;

    /// Delete a secret
    async fn delete(&self, path: &str) -> Result<(), SecretError>;

    /// Health check
    async fn health(&self) -> HealthStatus;
}
```

---

## 3. Built-in Providers

| Provider | Config Key | Use Case | Writable |
|---|---|---|---|
| `env` | `SERA_SECRET_<PATH>` env vars | Local development (default) | ❌ |
| `file` | `secrets/` directory with encrypted files | Development, simple deployments | ✅ |
| `vault` | HashiCorp Vault (Agent Injector pattern) | Enterprise on-prem | ✅ |
| `aws-sm` | AWS Secrets Manager | Enterprise cloud (AWS) | ✅ |
| `azure-kv` | Azure Key Vault | Enterprise cloud (Azure) | ✅ |
| `gcp-sm` | Google Secret Manager | Enterprise cloud (GCP) | ✅ |
| `custom` | Implement `SecretProvider` trait | Domain-specific | Implementation-dependent |

---

## 4. Secret References in Config

Secrets are referenced in config files using the `{ secret: "path/to/secret" }` syntax. References are resolved at runtime by the configured secret provider.

```yaml
connectors:
  - name: "discord-main"
    token: { secret: "connectors/discord-main/token" }

providers:
  - name: "openai"
    api_key: { secret: "providers/openai/api-key" }
```

**Secret values never appear in:**
- Config files (only references)
- Log output
- API responses (masked)
- Config read tool output (only reference paths returned)

---

## 5. Credential Injection

Tools that need external credentials receive them via the `CredentialBag` in `ToolContext`. The injection flow:

```
Tool call requested
  → pre_tool hook chain fires
    → "secret-injector" hook resolves credential mappings
    → Credentials injected into ToolContext.credentials
  → Tool executes with resolved credentials
  → Credentials are NOT logged or persisted
```

### Credential Mapping Configuration

```yaml
hooks:
  chains:
    pre_tool:
      - hook: "secret-injector"
        config:
          provider: "vault"
          mappings:
            GITHUB_TOKEN: "secrets/github/token"
            SLACK_WEBHOOK: "secrets/slack/webhook"
```

---

## 6. Side-Routed Secret Entry

A key capability: agents can **request secrets they don't yet have**. This enables a flow where:

1. Agent attempts an action requiring a credential
2. Agent discovers the credential is missing
3. Agent issues a tool call to request the secret
4. The request is routed to the operator (via approval system / HITL)
5. Operator enters the secret value through a secure channel (CLI, Web UI)
6. A secret **reference** is returned to the agent (never the raw value)
7. The secret is stored in the configured secret provider
8. Future tool calls resolve the credential automatically

### 6.1 Flow Diagram

```
Agent: "I need a GitHub token to proceed"
  → Agent calls secret_request tool
  → ApprovalSpec created with scope: SecretEntry
  → Routed to operator via HITL system
  → Operator enters secret value through secure UI
  → sera-secrets stores the value at the configured path
  → Reference path returned to agent
  → Agent updates its config (config_propose) to map the reference
  → Future tool calls auto-inject the credential
```

### 6.2 Secret Request Tool

```rust
pub struct SecretRequestTool;

impl Tool for SecretRequestTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "secret_request".to_string(),
            description: "Request a secret from an operator".to_string(),
            risk_level: RiskLevel::Admin,
            ..
        }
    }

    async fn execute(&self, input: ToolInput, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        // Creates an ApprovalSpec for secret entry
        // Routes to HITL system
        // Returns the secret reference path (NOT the value)
    }
}
```

### 6.3 Authorization

The side-routed entry flow is subject to full authorization:
- Agent must be authorized to call `secret_request` (tool policy + AuthZ)
- The approval routing determines who can enter the secret
- Hook chains can add additional authorization checks
- Audit trail records who requested, who entered, and what path was created

---

## 7. External Secret Provider (gRPC)

```protobuf
service SecretProviderService {
    rpc Resolve(SecretRef) returns (SecretValue);
    rpc ListPaths(SecretPathPrefix) returns (SecretPathList);
    rpc Health(Empty) returns (HealthResponse);
}
```

External secret providers can be used alongside or instead of built-in providers. They register with the gateway like other gRPC adapters.

---

## 8. Configuration

```yaml
sera:
  secrets:
    provider: "env"                     # env | file | vault | aws-sm | azure-kv | gcp-sm | custom

    # File provider config
    file:
      directory: "./secrets"
      encryption: "age"                 # age | gpg | none (dev only)
      key: { env: "SERA_SECRETS_KEY" }

    # Vault provider config
    vault:
      address: "https://vault.example.com"
      auth_method: "approle"
      role_id: { env: "VAULT_ROLE_ID" }
      secret_id: { env: "VAULT_SECRET_ID" }
      mount_path: "sera"

    # AWS provider config
    aws_sm:
      region: "us-east-1"
      prefix: "sera/"
```

---

## 9. Invariants

| # | Invariant | Enforcement |
|---|---|---|
| 14 | Secrets never in config | Reference-only in config files; resolved at runtime by provider |

---

## 10. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-config` | [SPEC-config](SPEC-config.md) | Secret references in config |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | Credential injection via ToolContext |
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | Secret-injector hook pattern |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Side-routed entry via approval system |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Authorization for secret access |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Secret resolution for connector tokens |

---

## 11. Open Questions

1. **Secret rotation** — How are secret rotations handled? Automatic re-resolution? Notification to affected agents?
2. **Secret versioning** — Do secret providers support versioned secrets? Can you roll back to a previous secret value?
3. **Secret auditing** — Is every secret access logged? What's the granularity (per-resolution vs. per-session)?
4. **File encryption** — What encryption scheme does the file provider use? Age? GPG? Something else?
5. **Secret sharing between agents** — Can multiple agents share the same secret reference, or are secrets scoped per-agent?
6. **Secret expiry** — Do secrets have TTLs? Can the system automatically request renewal?

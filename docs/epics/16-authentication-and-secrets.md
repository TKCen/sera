# Epic 16: Authentication, Identity & Secrets

## Overview

Two foundational concerns that must be right from the start: who the human operator is, and how credentials are stored and injected into agents. Authentication uses OIDC as the primary protocol — operator identity is real from day one, making the audit trail meaningful. Secrets management uses a pluggable provider interface with an encrypted PostgreSQL implementation as the default — no new infrastructure required, swappable for Vault or cloud secret managers later.

## Context

- See `docs/ARCHITECTURE.md` → Authentication, Secrets Management
- Agent identity (JWT issued by sera-core) and operator identity (token issued by IdP) are distinct and handled by separate validators
- Secrets are **never** returned in API responses or stored in `resolved_capabilities` — injected at container spawn time only
- This epic is a prerequisite for: meaningful audit trail entries, per-operator permission grants, multi-user deployments

## Dependencies

- Epic 01 (Infrastructure) — Authentik or external IdP must be reachable; `SECRETS_MASTER_KEY` env var
- Epic 02 (Agent Manifest) — `secrets.access` in capability model
- Epic 03 (Docker Sandbox) — secret injection at container spawn

---

## Stories

### Story 16.1: OIDC provider integration (sera-core)

**As** sera-core
**I want** to validate OIDC tokens issued by a configured identity provider
**So that** every human operator action carries a verified identity

**Acceptance Criteria:**
- [ ] `OIDCAuthProvider` configured via env vars: `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `OIDC_AUDIENCE`
- [ ] On startup: fetches JWKS from `{OIDC_ISSUER_URL}/.well-known/jwks.json`; refreshes JWKS on key rotation (via `kid` mismatch)
- [ ] `Authorization: Bearer {token}` header accepted on all protected API endpoints
- [ ] Token validated: signature, expiry, `iss` (matches configured issuer), `aud` (matches configured audience)
- [ ] `OperatorIdentity` extracted from claims: `sub` (stable unique ID), `email`, `name`, `preferred_username`, `groups` (for role mapping)
- [ ] Token validation adds < 2ms to request handling (JWKS cached in memory, no per-request HTTP)
- [ ] Invalid/expired token → 401 with `{ error: 'invalid_token', hint: 'human-readable reason' }`
- [ ] OIDC provider is the `AuthPlugin` default — other plugins (API key) layered alongside it

**Technical Notes:**
- **OIDC client library: `openid-client` v6** — handles JWKS discovery, key rotation, PKCE, token refresh, and device flow. Do not implement JWT validation manually.
- **JWT operations: `jose` v5** — used for signing and verifying internal agent identity tokens (issued by `IdentityService` at container spawn). Replaces `jsonwebtoken` — actively maintained, standards-compliant, native ES modules.
- `groups` claim name is configurable: `OIDC_GROUPS_CLAIM` (default: `groups`) — different IdPs use different claim names
- If `OIDC_ISSUER_URL` is not set, sera-core starts in API-key-only mode with a prominent warning
- **Local development auth**: use API-key-only mode with `SERA_BOOTSTRAP_API_KEY` (Story 16.3) — no mock OIDC server needed during development. Authentik (Story 16.2) is for staging/production homelab use.

---

### Story 16.2: Authentik reference setup for homelab

**As an** operator deploying SERA on a homelab
**I want** a ready-to-use Authentik configuration alongside SERA
**So that** I have a working OIDC provider without configuring one from scratch

**Acceptance Criteria:**
- [ ] `docker-compose.auth.yaml` override file adds Authentik (server + worker + Redis + PostgreSQL-for-authentik) to the stack
- [ ] Authentik pre-configured as an OIDC provider for SERA via a bootstrap config script
- [ ] `sera-core` env vars for Authentik pre-filled in `.env.example` (commented out, clearly labelled)
- [ ] README section: "Authentication setup" documents three paths:
  1. Bring your own OIDC provider (any compliant IdP — Keycloak, Google, GitHub, etc.)
  2. Use the bundled Authentik setup
  3. API-key-only mode (no IdP, single-user, not recommended for production)
- [ ] Authentik service is opt-in — the base `docker-compose.yaml` does not include it

**Technical Notes:**
- Authentik: `ghcr.io/goauthentik/server:latest` — document pinning to a specific version
- SERA should be tested against at least: Authentik, Keycloak, and GitHub OAuth2 (OIDC-compatible)

---

### Story 16.3: API key authentication (machine/CLI fallback)

**As an** operator using the CLI or automation scripts
**I want** to authenticate with a static API key
**So that** non-interactive tools work without OIDC device flow

**Acceptance Criteria:**
- [ ] `api_keys` table: `id` (UUID), `name`, `key_hash` (bcrypt), `owner_sub` (maps to an operator identity), `roles` (array), `created_at`, `expires_at` (nullable), `last_used_at`, `revoked_at`
- [ ] API keys accepted via `Authorization: Bearer {key}` — same header as OIDC tokens
- [ ] `APIKeyAuthProvider` distinguishes from OIDC tokens by format (OIDC tokens are JWTs; API keys are opaque strings — e.g. `sera_` prefix)
- [ ] `POST /api/auth/api-keys` creates an API key — requires OIDC-authenticated operator (or bootstrap key)
- [ ] `GET /api/auth/api-keys` lists keys for the authenticated operator (hashes never returned)
- [ ] `DELETE /api/auth/api-keys/:id` revokes a key (sets `revoked_at`)
- [ ] Bootstrap key: `SERA_BOOTSTRAP_API_KEY` env var accepted on first start only — used to create the first OIDC user and API keys; disabled after first use
- [ ] API key auth records `authMethod: 'api-key'` in `OperatorIdentity` — visible in audit trail

---

### Story 16.4: Operator identity and RBAC

**As** sera-core
**I want** a role-based access control model for human operators
**So that** different operators have appropriate access levels and audit trail entries carry real identity

**Acceptance Criteria:**
- [ ] Four built-in roles: `admin`, `operator`, `viewer`, `agent-runner`
  - `admin`: full access to all API endpoints
  - `operator`: manage agents, circles, schedules, providers, skills; cannot manage operators or system config
  - `viewer`: read-only on all resources; cannot start/stop agents or trigger actions
  - `agent-runner`: can start/stop agents and trigger schedules; cannot create/modify/delete resources
- [ ] Roles mapped from IdP groups via `OIDC_ROLE_MAPPING` config: `{ "sera-admins": "admin", "sera-ops": "operator" }`
- [ ] Role enforced per endpoint via middleware — 403 if role insufficient
- [ ] `OperatorIdentity` available throughout the request lifecycle via request context
- [ ] `OperatorIdentity.sub` used as `granted_by` in `capability_grants`, `created_by` in `secrets`, and `actor_id` in `audit_trail` for operator-sourced events
- [ ] `GET /api/auth/me` returns the authenticated operator's identity and roles
- [ ] Custom roles: `POST /api/auth/roles` — admin only; assigns specific endpoint permissions rather than a preset bundle

---

### Story 16.5: Web UI authentication flow

**As an** operator
**I want** to log in to the sera-web UI via my OIDC provider
**So that** the UI is protected and my identity is carried into all actions

**Acceptance Criteria:**
- [ ] Unauthenticated UI requests redirect to `GET /api/auth/login` which initiates OIDC authorization code + PKCE flow
- [ ] Callback handler at `GET /api/auth/callback` exchanges code for tokens, creates a session
- [ ] Session stored server-side (encrypted cookie or DB-backed session) — access token not stored in `localStorage`
- [ ] Session includes: `operatorSub`, `email`, `name`, `roles`, `accessTokenExpiry`
- [ ] UI refresh: sera-core transparently refreshes the access token using the refresh token before expiry
- [ ] `POST /api/auth/logout` revokes session, redirects to IdP logout endpoint
- [ ] UI shows logged-in operator name and role in the top navigation bar
- [ ] Role-gated UI elements: `operator`-only actions (create agent, delete) hidden from `viewer` role — not just disabled
- [ ] Session timeout configurable: `SESSION_MAX_AGE_SECONDS` (default: 28800 — 8 hours)

---

### Story 16.6: CLI authentication (OIDC device flow)

**As an** operator using the `sera` CLI
**I want** to authenticate interactively via my OIDC provider
**So that** CLI commands carry my real identity without embedding credentials in config files

**Acceptance Criteria:**
- [ ] `sera auth login` initiates OIDC device authorization flow: prints a URL + user code, polls for completion
- [ ] On completion: tokens stored in `~/.sera/credentials` (encrypted with OS keychain or file permissions 600)
- [ ] `sera auth logout` revokes and removes stored tokens
- [ ] `sera auth status` shows current authenticated identity, token expiry, roles
- [ ] All CLI commands that call sera-core API use the stored access token; refresh automatically before expiry
- [ ] Non-interactive mode: `--api-key {key}` flag or `SERA_API_KEY` env var accepted on any command
- [ ] `sera auth login --service-account` creates or retrieves a long-lived API key for the current identity (for use in scripts)

---

### Story 16.7: SecretsProvider interface

**As** a developer or operator
**I want** a pluggable secrets provider interface
**So that** SERA can use different secret backends (PostgreSQL, Vault, cloud providers) without code changes

**Acceptance Criteria:**
- [ ] `SecretsProvider` TypeScript interface:
  ```typescript
  interface SecretsProvider {
    id: string
    get(name: string, context: SecretAccessContext): Promise<string | null>
    set(name: string, value: string, metadata?: SecretMetadata): Promise<void>
    delete(name: string): Promise<void>
    list(filter?: SecretFilter): Promise<SecretMetadata[]>
    rotate(name: string, newValue: string): Promise<void>
    healthCheck(): Promise<boolean>
  }

  interface SecretAccessContext {
    agentId: string
    agentName: string
  }
  ```
- [ ] `SecretsManager` singleton holds the active provider, resolved from `SECRETS_PROVIDER` env var (`postgres` default)
- [ ] `SecretsManager.get()` enforces access control: checks secret's `allowedAgents` list against `context.agentId` before returning value
- [ ] Secret values **never** logged, never returned via REST API (only metadata)
- [ ] `SecretsProvider` registered in plugin system (Epic 15) — community-buildable Vault/cloud provider plugins
- [ ] `GET /api/secrets` returns metadata only: name, description, allowed agents, created/updated timestamps
- [ ] `POST /api/secrets` creates/updates a secret — admin/operator role required
- [ ] `DELETE /api/secrets/:name` — admin role required; soft-delete with 24h recovery window before permanent deletion

---

### Story 16.8: PostgreSQL secrets provider (default)

**As an** operator
**I want** secrets stored encrypted in PostgreSQL without any additional infrastructure
**So that** the default installation requires no external secrets manager

**Acceptance Criteria:**
- [ ] `secrets` table:
  ```sql
  CREATE TABLE secrets (
    id           UUID PRIMARY KEY,
    name         TEXT UNIQUE NOT NULL,
    encrypted_value BYTEA NOT NULL,
    iv           BYTEA NOT NULL,       -- AES-256-GCM IV (96-bit, unique per secret)
    description  TEXT,
    allowed_agents TEXT[],             -- agent names that may access this secret
    tags         TEXT[],
    created_by   TEXT,
    created_at   TIMESTAMPTZ,
    updated_at   TIMESTAMPTZ,
    rotated_at   TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ,          -- nullable
    deleted_at   TIMESTAMPTZ           -- soft delete
  );
  ```
- [ ] Encryption: AES-256-GCM using `SECRETS_MASTER_KEY` env var (required; sera-core refuses to start if absent)
- [ ] Master key loaded from env — never stored in DB, never logged
- [ ] Each secret encrypted with a fresh random 96-bit IV — IV stored alongside ciphertext
- [ ] `POST /api/secrets/rotate-master-key` re-encrypts all secrets with a new master key — admin only; transactional (all or nothing)
- [ ] Secret access logged in audit trail: name (not value), requesting agent ID, timestamp, granted/denied
- [ ] Unit tests: encrypt → store → retrieve → decrypt round-trip; access denied for agent not in `allowed_agents`; missing master key → startup failure
- [ ] Secret `exposure` field: `per-call` (default) | `agent-env`; stored on the secret record
  - `per-call`: secret value is resolved by `CredentialResolver` at tool execution time only; never injected into container startup environment
  - `agent-env`: secret value injected as `SERA_SECRET_{NAME}` at container spawn (opt-in; requires explicit `exposure: agent-env` in the secret record)
- [ ] `POST /api/secrets` accepts `exposure` field; defaults to `per-call`
- [ ] `agent-env` secrets that were previously `per-call` can be changed — takes effect on next agent restart

**Technical Notes:**
- `SECRETS_MASTER_KEY` should be a 32-byte (256-bit) random hex or base64 string
- Key derivation: if the provided key is a passphrase rather than raw bytes, derive via PBKDF2-SHA256 with a stored salt
- Recovery: if master key is lost, all secrets must be re-entered. Document this prominently. Recommend operators store the master key in a password manager or physical safe.

---

### Story 16.9: Secret injection at agent spawn

**As** sera-core
**I want** to inject secrets into agent containers as environment variables at spawn time
**So that** agents can access credentials without those credentials ever appearing in config files, logs, or API responses

**Acceptance Criteria:**
- [ ] At `SandboxManager.spawn()`: read agent's resolved `capabilities.secrets.access` list
- [ ] For each secret name: call `SecretsManager.get(name, { agentId, agentName })`
- [ ] Access check in `SecretsProvider`: agent must be in secret's `allowed_agents` — denied secrets logged as warnings, not errors (agent starts without them)
- [ ] Injected as env vars: `SERA_SECRET_{NAME_UPPERCASED}=value` — e.g. `capabilities.secrets.access: [GITHUB_TOKEN]` → `SERA_SECRET_GITHUB_TOKEN=...`
- [ ] Secret values **never** written to `resolved_capabilities` JSONB, container labels, or any log line
- [ ] Docker API call for secret injection uses the `Env` container config array — values in memory only, not written to disk by Docker
- [ ] Audit record: `{ action: 'secret.injected', secretName, agentId }` — name only, no value
- [ ] After spawn: decrypted secret values zeroised from sera-core process memory (Buffer zeroed before GC)

---

### Story 16.10: Permission grant identity

**As an** operator
**I want** all runtime permission grants to carry my verified identity
**So that** the audit trail shows exactly who granted access to what

**Acceptance Criteria:**
- [ ] `POST /api/permission-requests/:id/decision` requires authenticated operator (OIDC or API key)
- [ ] `OperatorIdentity.sub` and `email` stored in `capability_grants.granted_by` — not just a string label
- [ ] Audit trail entry for a grant includes: `actorId` (operator sub), `actorEmail`, `actorAuthMethod` (oidc/api-key)
- [ ] Grants made via the CLI carry the CLI-authenticated operator identity
- [ ] UI permission grant dialog shows the requesting agent, the requested resource, and the logged-in operator's name ("You are granting as: alice@example.com")
- [ ] `GET /api/agents/:id/grants` includes `grantedBy` (email) and `grantedAt` for each grant

---

### Story 16.11: Secret rotation propagation

**As an** operator
**I want** running agents and MCP servers to be notified when a secret they use has been rotated
**So that** stale credentials are not silently used after rotation

**Acceptance Criteria:**
- [ ] On `POST /api/secrets/rotate` (or master key rotation): sera-core identifies all agents and MCP servers that reference the rotated secret via `allowed_agents` or `agent_service_identities`
- [ ] For `per-call` exposure secrets: rotation is effective immediately on the next tool call (no notification needed — `CredentialResolver` always fetches the current value)
- [ ] For `agent-env` exposure secrets: rotation cannot take effect without a container restart; sera-core:
  - Publishes `system.secrets.rotated` Centrifugo event: `{ secretName, affectedAgentIds, requiresRestart: true }`
  - Creates a notification via `ChannelRouter` (Epic 18) with severity `warning`: "Secret {name} has been rotated. Affected agents must be restarted to pick up the new value."
  - `GET /api/agents/:id` includes `pendingSecretRotations: [secretName]` if the agent has stale `agent-env` secrets
- [ ] `POST /api/agents/:id/restart?applySecretRotations=true` restarts the agent with current secret values
- [ ] Audit record on rotation: `{ action: 'secret.rotated', secretName, affectedAgents: N, requiresRestart: N }`

# Authentication & OIDC

SERA supports three authentication modes, from simple to enterprise.

## Mode 1: API Key Only (Default)

The simplest setup. Set `SERA_BOOTSTRAP_API_KEY` in `.env` and use it as a Bearer token:

```bash
curl -H "Authorization: Bearer sera_bootstrap_dev_123" \
  http://localhost:3001/api/agents
```

The web dashboard reads the dev API key from `VITE_DEV_API_KEY` in dev mode.

## Mode 2: Bring Your Own IdP

Connect any OIDC-compliant identity provider (Keycloak, Auth0, Okta, Azure AD, etc.):

```bash title=".env"
OIDC_ISSUER_URL=https://your-idp.example.com/realms/sera
OIDC_CLIENT_ID=sera-web
OIDC_CLIENT_SECRET=your-client-secret
OIDC_AUDIENCE=sera-api
OIDC_GROUPS_CLAIM=groups
OIDC_ROLE_MAPPING={"sera-admins":"admin","sera-ops":"operator","sera-viewers":"viewer"}
```

### Role Mapping

SERA maps IdP groups to internal roles:

| Role       | Permissions                                                 |
| ---------- | ----------------------------------------------------------- |
| `admin`    | Full access — manage agents, secrets, providers, channels   |
| `operator` | Manage agents and approve permissions, no secret management |
| `viewer`   | Read-only access to dashboards and audit logs               |

### Token Flow

1. User clicks **Login** in sera-web
2. PKCE flow redirects to IdP
3. IdP returns authorization code
4. sera-web exchanges code via `POST /api/auth/oidc/callback`
5. sera-core validates the token, creates a session, returns an opaque `sess_*` token
6. The OIDC access token stays server-side only — never reaches the browser

## Mode 3: Bundled Authentik

SERA includes a Docker Compose overlay for [Authentik](https://goauthentik.io/), a self-hosted IdP:

```bash
bun run prod:auth:up
```

This starts Authentik alongside the SERA stack. Configure it at `http://localhost:9000`.

```bash title=".env"
OIDC_ISSUER_URL=http://authentik-server:9000/application/o/sera/
OIDC_CLIENT_ID=sera-web
OIDC_CLIENT_SECRET=<from authentik>
AUTHENTIK_SECRET_KEY=change-me-to-a-long-random-string
AUTHENTIK_POSTGRESQL_PASSWORD=authentik-db-password
```

## CLI Authentication

The Go CLI supports OIDC device flow for terminal-based login:

```bash
sera auth login              # OIDC device flow
sera auth login --service-account  # Long-lived API key
sera auth status             # Show current identity
sera auth logout             # Revoke credentials
```

Credentials are stored in `~/.sera/credentials` (mode 0600).

## Session Management

| Setting                   | Default                      | Description           |
| ------------------------- | ---------------------------- | --------------------- |
| `SESSION_MAX_AGE_SECONDS` | 28800 (8 hours)              | Session expiry        |
| JWKS cache                | Auto-refresh on kid mismatch | Key rotation handling |

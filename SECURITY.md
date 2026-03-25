# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in SERA, please report it responsibly:

1. **Do not** open a public GitHub issue for security vulnerabilities
2. Email: [Create a private security advisory](https://github.com/TKCen/sera/security/advisories/new)
3. Include: description, reproduction steps, and potential impact

We will acknowledge receipt within 48 hours and aim to provide a fix within 7 days for critical issues.

## Secrets Management

SERA uses environment variables for all secrets. **Never commit real credentials to the repository.**

### Required secrets for production:

| Variable | Purpose |
|---|---|
| `SECRETS_MASTER_KEY` | AES-256-GCM encryption key for stored secrets (64 hex chars) |
| `SERA_BOOTSTRAP_API_KEY` | Initial API key for operator authentication |
| `DATABASE_URL` | PostgreSQL connection string |

### Optional secrets:

| Variable | Purpose |
|---|---|
| `OPENAI_API_KEY` | OpenAI API access |
| `ANTHROPIC_API_KEY` | Anthropic API access |
| `GOOGLE_API_KEY` | Google AI Studio access |
| `CENTRIFUGO_API_KEY` | Centrifugo server-to-server auth |

### Setup

1. Copy `.env.example` to `.env`
2. Generate a strong `SECRETS_MASTER_KEY`: `openssl rand -hex 32`
3. Generate a strong `SERA_BOOTSTRAP_API_KEY`: `openssl rand -base64 32`
4. Never use the default development values in production

## Architecture Security

- **Agent sandboxing**: Each agent runs in an isolated Docker container with resource limits
- **Capability-based access**: Agents only access what their policy permits
- **Egress filtering**: Outbound network traffic filtered by Squid proxy with per-agent ACLs
- **Audit trail**: All agent actions are recorded with Merkle hash-chain integrity
- **Delegation tokens**: Operator-to-agent trust with scope intersection validation

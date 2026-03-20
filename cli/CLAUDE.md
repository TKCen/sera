# sera CLI

Go-based CLI for interacting with the SERA platform. Implements Story 16.6 (OIDC device flow) from Epic 16.

## Language and tooling

- **Language:** Go (no external dependencies — uses stdlib only)
- **Build:** `go build -o sera.exe .` (Windows) / `go build -o sera .` (Linux/macOS)
- **Test:** `go test ./...`

```bash
cd D:/projects/homelab/sera/cli && go build -o sera.exe .
```

## Commands

| Command | Description |
|---|---|
| `sera auth login` | OIDC device authorization flow |
| `sera auth login --service-account` | Create long-lived API key after device flow |
| `sera auth logout` | Revoke session + delete `~/.sera/credentials` |
| `sera auth status` | Show current identity and token expiry |

## Credentials file

`~/.sera/credentials` — JSON, mode 0600:
```json
{ "apiKey": "...", "issuer": "...", "expiresAt": "2026-01-01T00:00:00Z" }
```

## Configuration

- `SERA_API_URL` — sera-core API base (default: `http://localhost:3001`)
- `OIDC_ISSUER_URL` — OIDC issuer (resolved from sera-core's `/api/auth/oidc-config` first)
- `OIDC_CLIENT_ID` — client ID (default: `sera-web`)
- `SERA_API_KEY` — bypass stored credentials

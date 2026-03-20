# sera-e2e

End-to-end tests for SERA using Playwright.

## Setup

```bash
cd e2e
npm install
npx playwright install chromium
```

## Running tests

```bash
# Dev stack (default) — requires dev compose to be running
npm test

# Explicit modes
npm run test:dev       # docker-compose.yaml + docker-compose.dev.yaml
npm run test:api-key   # docker-compose.yaml only (no OIDC, + port mapping needed)
npm run test:oidc      # docker-compose.yaml + docker-compose.auth.yaml
```

## Key environment variables

| Variable | Default | Purpose |
|---|---|---|
| `E2E_STACK_MODE` | `dev` | Which login/stack variant to test |
| `SERA_WEB_URL` | `http://localhost:3000` (dev) / `http://localhost:8080` | sera-web base URL |
| `SERA_API_URL` | `http://localhost:3001` | sera-core base URL |
| `SERA_API_KEY` | `sera_bootstrap_dev_123` | Bootstrap API key |
| `E2E_OIDC_USER` | `akadmin` | Authentik username (oidc mode) |
| `E2E_OIDC_PASSWORD` | _(required)_ | Authentik password (oidc mode) |

## Stack mode reference

| Mode | Compose files | Web port | Login path |
|---|---|---|---|
| `dev` | base + dev overlay | `3000` | One-button dev key (VITE_DEV_API_KEY) |
| `api-key` | base only | `8080`* | Manual API key input |
| `oidc` | base + auth overlay | `8080`* | OIDC PKCE via Authentik |

*`docker-compose.yaml` does not expose sera-web's port 80 by default. For `api-key` and `oidc` modes add a `ports: ["8080:80"]` entry to `sera-web` in a local override compose file.

## Test files

| File | What it tests |
|---|---|
| `tests/journey-01-stack-health.spec.ts` | All services healthy, SERA API reachable |
| `tests/journey-02-login.spec.ts` | All three login paths: dev key, API key, OIDC |

## Learnings

- **`test.skip()` with a condition must be the first statement in the test body** — Playwright evaluates `test.skip(condition)` at collection time; placing it after awaits can cause flaky skips.
- **sessionStorage is per-origin** — `page.addInitScript(() => sessionStorage.clear())` in `beforeEach` reliably clears state before each test without needing a full browser restart.

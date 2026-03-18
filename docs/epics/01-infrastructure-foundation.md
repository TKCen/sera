# Epic 01: Infrastructure Foundation

## Overview

Establish the complete Docker Compose stack that all other epics build on. Every service — sera-core, sera-web, Centrifugo, PostgreSQL, Qdrant, and LiteLLM — must be wired together with correct networking, health checks, environment management, and a friction-free local development experience. This is the foundation everything else runs on.

## Context

- See `docs/ARCHITECTURE.md` → System Overview, Provider Gateway: LiteLLM
- Two Docker networks: `sera_net` (internal services) and `agent_net` (agent containers + MCP servers)
- LiteLLM is a dumb routing socket; sera-core holds all governance
- Local-first defaults: LM Studio and Ollama should work out of the box without any cloud API keys

## Dependencies

None. This epic has no upstream dependencies.

---

## Stories

### Story 1.1: Base Docker Compose stack

**As an** operator
**I want** a single `docker compose up -d` to start all SERA services
**So that** I can have a fully running system without manual configuration steps

**Acceptance Criteria:**
- [ ] `docker-compose.yaml` defines: `sera-core`, `sera-web`, `centrifugo`, `sera-db` (PostgreSQL + pgvector), `qdrant`, `litellm`
- [ ] All services connect to `sera_net` bridge network
- [ ] `agent_net` is declared as an external network (created separately, used by spawned agent containers)
- [ ] All services have `restart: unless-stopped`
- [ ] All services have `healthcheck` definitions with appropriate intervals
- [ ] `sera-core` depends on `sera-db` and `qdrant` via `condition: service_healthy`
- [ ] `sera-web` depends on `sera-core` via `condition: service_healthy`
- [ ] Stack starts cleanly on a machine with no prior state

**Technical Notes:**
- `agent_net` must be external because sera-core creates agent containers dynamically at runtime; the network must pre-exist
- pgvector image: `pgvector/pgvector:pg15`
- Qdrant image: `qdrant/qdrant:latest`

---

### Story 1.2: LiteLLM provider gateway container

**As an** operator
**I want** LiteLLM running as a managed service in the stack
**So that** sera-core can route LLM calls to any provider without managing API keys or provider-specific SDKs

**Acceptance Criteria:**
- [ ] `litellm` service uses `ghcr.io/berriai/litellm:main-stable` (not `latest`)
- [ ] Configuration loaded from `./litellm/config.yaml` volume mount
- [ ] `LITELLM_MASTER_KEY` environment variable set from `.env` — this is the only key sera-core uses
- [ ] Upstream provider API keys (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`) passed as optional env vars — services start without them
- [ ] LiteLLM shares `sera-db` PostgreSQL instance (separate database `litellm_db`)
- [ ] Health check on `GET /health`
- [ ] sera-core `LLM_BASE_URL` points to `http://litellm:4000/v1`

**Technical Notes:**
- Use `main-stable` tag — published weekly after 3-day production validation
- LiteLLM's own governance features (virtual keys, team budgets) are intentionally not used; sera-core owns governance
- LiteLLM should be treated as an internal implementation detail; operators interact with providers via SERA's API

---

### Story 1.3: LiteLLM base configuration (local-first defaults)

**As an** operator
**I want** LiteLLM pre-configured for local LLM providers (LM Studio, Ollama)
**So that** SERA works out of the box on a homelab without requiring cloud API keys

**Acceptance Criteria:**
- [ ] `litellm/config.yaml` committed to repository with working defaults
- [ ] Default model list includes: LM Studio (`http://host.docker.internal:1234/v1`), Ollama (`http://host.docker.internal:11434`)
- [ ] Cloud providers (OpenAI, Anthropic) defined but gated on env var presence — no error if key is absent
- [ ] `routing_strategy: latency-based-routing` set as default
- [ ] Fallback chain defined: local → cloud if local is unavailable
- [ ] `num_retries: 2`, `timeout: 120` set globally
- [ ] `drop_params: true` set to handle model-specific param differences silently

**Technical Notes:**
- Cloud provider entries should use `os.environ/OPENAI_API_KEY` syntax so LiteLLM skips them when the var is unset
- The config file is the source of truth for routing strategy; model additions/removals happen via API at runtime

---

### Story 1.4: Environment variable management

**As a** developer
**I want** a documented `.env.example` with all required and optional variables
**So that** I can set up a new development environment without hunting through service definitions

**Acceptance Criteria:**
- [ ] `.env.example` at repo root with every env var used across all services
- [ ] Variables grouped by service with comments explaining purpose and whether required/optional
- [ ] Sensitive vars (API keys, DB passwords, JWT secrets) clearly marked — never committed with real values
- [ ] `.env` added to `.gitignore`
- [ ] `docker-compose.yaml` uses `${VAR:-default}` syntax so stack starts with sane defaults even without a `.env`
- [ ] `README.md` contains a "Getting Started" section referencing `.env.example`

**Technical Notes:**
- Key variables: `LITELLM_MASTER_KEY`, `JWT_SECRET`, `DATABASE_URL`, `OPENAI_API_KEY` (optional), `ANTHROPIC_API_KEY` (optional), `LM_STUDIO_URL` (optional override), `NEXT_PUBLIC_CENTRIFUGO_URL`
- Default DB credentials in `.env.example` should be obviously insecure placeholders

---

### Story 1.5: Database initialisation and migrations

**As a** developer
**I want** the database schema applied automatically on first start
**So that** a fresh deployment requires no manual SQL steps

**Acceptance Criteria:**
- [ ] Migration system in place for sera-core's PostgreSQL schema (e.g. `node-pg-migrate` or plain ordered SQL files)
- [ ] Migrations run automatically at sera-core startup before the HTTP server starts accepting requests
- [ ] pgvector extension enabled as part of initial migration
- [ ] Migrations are idempotent — running twice has no effect
- [ ] Migration files are committed and version-controlled
- [ ] Schema covers: `agent_instances`, `chat_sessions`, `chat_messages`, `embeddings`, `token_usage`, `token_quotas`, `usage_events`, `audit_trail`, `schedules`

---

### Story 1.6: Local development workflow

**As a** developer
**I want** a fast local development loop that doesn't require rebuilding Docker images for every code change
**So that** I can iterate quickly on sera-core and sera-web

**Acceptance Criteria:**
- [ ] `docker-compose.dev.yaml` override file that mounts source directories and runs services in watch/hot-reload mode
- [ ] sera-core runs with `ts-node --watch` (or equivalent) in dev mode — code changes restart the process without image rebuild
- [ ] sera-web runs with Vite dev server (or Next.js dev) with HMR
- [ ] Infra services (DB, Qdrant, Centrifugo, LiteLLM) run from pre-built images — only application code is mounted
- [ ] `docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up` documented in README
- [ ] `agent_net` creation documented — `docker network create agent_net` as a prerequisite step

---

### Story 1.7: Backup and restore (P2 — deferred)

**As an** operator
**I want** a documented procedure to back up and restore a SERA instance
**So that** I can migrate to new hardware, recover from failures, and snapshot state before upgrades

> **Status:** Deferred. Stub story to prevent architectural foreclosure.

**Acceptance Criteria (minimum viable, when implemented):**
- [ ] `sera backup` CLI command exports: PostgreSQL dump, Qdrant collection snapshots, `skills/`, `skill-packs/`, `mcp-servers/` directories, `.env` (or env var reference list, not values)
- [ ] `sera restore <backup-file>` applies the backup to a fresh or existing instance
- [ ] Backup includes a manifest with: sera-core version, backup timestamp, included components
- [ ] Secrets (encrypted at rest in PostgreSQL) are included in the DB dump — master key must be preserved separately; documented prominently
- [ ] Backup/restore does not require stopping the instance (online backup via `pg_dump`)

**Technical Notes:**
- Workspace files (bind-mounted agent working directories) are explicitly excluded — too large and agent-specific; document this exclusion
- The `.env` file must not be included in the backup archive as it may contain plaintext credentials; instead include a list of required variable names

---

### Story 1.8: Instance identity (P3 — deferred)

**As** a SERA instance
**I want** a stable, globally unique instance identity
**So that** audit trail entries, federation messages, and telemetry are attributable to a specific instance even in multi-instance deployments

> **Status:** Deferred. Stub story to prevent architectural foreclosure.

**Acceptance Criteria (minimum viable, when implemented):**
- [ ] `instance_identity` table: single row with `id` (UUID, generated on first start), `name` (operator-configurable), `created_at`
- [ ] Instance ID included in all audit trail entries as `instanceId`
- [ ] `GET /api/system/identity` returns `{ id, name, version, createdAt }` — unauthenticated (for federation discovery)
- [ ] Instance name configurable via `SERA_INSTANCE_NAME` env var (default: hostname)

---

### Story 1.9: Graceful upgrade path (P3 — deferred)

**As an** operator
**I want** a documented and tested upgrade procedure for sera-core
**So that** I can apply new versions without data loss or extended downtime

> **Status:** Deferred. Stub story to prevent architectural foreclosure.

**Acceptance Criteria (minimum viable, when implemented):**
- [ ] Migration files are forward-only and numbered sequentially — no destructive column drops in the same migration that adds a replacement
- [ ] `sera-core` checks pending migrations on startup and refuses to start if the DB schema version is ahead of the binary (downgrade guard)
- [ ] `CHANGELOG.md` documents breaking changes to `.env` variables, API contracts, and manifest schemas between versions
- [ ] Upgrade procedure documented in `docs/UPGRADE.md`: pull new images, run `docker compose up -d`, monitor logs
- [ ] At least one E2E upgrade test: start with version N, insert data, upgrade to N+1, verify data intact and API functional

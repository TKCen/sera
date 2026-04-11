# Deployment Guide

Deploy SERA for production or homelab use.

## Architecture Overview

```
┌─────────────┐     ┌──────────┐     ┌─────────────┐
│  sera-web   │────→│sera-core │────→│ sera-db     │
│  (dashboard)│     │  (API)   │     │ (PostgreSQL)│
└─────────────┘     └────┬─────┘     └─────────────┘
                         │
               ┌─────────┼─────────┐
               │         │         │
        ┌──────┴──┐ ┌────┴────┐ ┌──┴──────────┐
        │centrifugo│ │ qdrant  │ │egress-proxy │
        │(realtime)│ │(vectors)│ │  (Squid)    │
        └─────────┘ └─────────┘ └─────────────┘
                         │
               ┌─────────┴─────────┐
               │  Agent Containers  │
               │  (sera-agent-*)    │
               └───────────────────┘
```

## Requirements

- Docker Engine 24+ with Compose v2
- 4GB+ RAM (8GB+ recommended for local LLMs)
- 10GB disk for Docker images and agent workspaces

## Production Deployment

### 1. Environment Configuration

```bash
cp .env.example .env
```

**Required settings:**

| Variable | Description | Example |
|----------|-------------|---------|
| `SERA_BOOTSTRAP_API_KEY` | API authentication key | `sera_prod_<random>` |
| `JWT_SECRET` | JWT signing secret (32+ chars) | `<random 64 char hex>` |
| `SECRETS_MASTER_KEY` | Encryption key for secrets store (32-byte hex) | `<random 64 char hex>` |
| `DATABASE_URL` | PostgreSQL connection string | `postgresql://user:pass@host:5432/sera` |

**LLM Configuration:**

For local LLMs (LM Studio / Ollama):
```env
LLM_BASE_URL=http://host.docker.internal:1234/v1
LLM_MODEL=qwen3.5-35b-a3b
```

For cloud providers:
```env
OPENAI_API_KEY=sk-...
# or ANTHROPIC_API_KEY, GOOGLE_API_KEY, GEMINI_API_KEY
```

Cloud API keys are ingested into the encrypted secrets store on first startup — the env var is no longer needed after that.

### 2. Start the Stack

```bash
docker compose up -d
```

For development with hot-reload:
```bash
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d
```

### 3. Verify Health

```bash
curl -H "Authorization: Bearer <your-key>" http://localhost:3001/api/health
```

All services should report healthy within 30 seconds.

### 4. Access the Dashboard

Open `http://localhost:3000` (or your configured domain).

## Security Considerations

### Network Isolation

SERA uses two Docker networks:
- **sera_net**: Internal communication between core services
- **agent_net**: Agent containers + egress proxy (network-isolated from core)

Agent containers cannot reach core services directly on sera_net — they communicate only through the LLM proxy and tool proxy endpoints.

### Secret Management

- API keys are encrypted at rest using AES-256-GCM in PostgreSQL
- `SECRETS_MASTER_KEY` must be set for encryption — without it, keys are stored in plaintext (with a warning)
- Secrets are injected into agent containers at spawn time via environment variables, never exposed in API responses

### Agent Sandboxing

Each agent runs in an isolated Docker container with:
- **Tier-based resource limits** (CPU, memory, network)
- **Filesystem isolation** — workspace bind-mounted at `/workspace`
- **Network control** — egress proxy with per-agent ACLs
- **No Docker socket access** — agents cannot create containers directly

## Backup

### Database

```bash
docker exec sera-db pg_dump -U sera_user sera_db > backup.sql
```

### Agent Workspaces

```bash
tar -czf workspaces-backup.tar.gz workspaces/
```

### Memory Blocks

```bash
tar -czf memory-backup.tar.gz memory/
```

## Upgrading

```bash
git pull
docker compose down
docker compose build
docker compose up -d
```

Migrations run automatically on startup — no manual step needed.

## Monitoring

- **Health endpoint**: `GET /api/health` — overall system status
- **Provider health**: `GET /api/providers/:model/health` — per-model circuit breaker state
- **Metrics**: `GET /api/metering/usage` — token usage by agent/day
- **Audit**: `GET /api/audit` — tamper-proof event log
- **Real-time**: Centrifugo dashboard at `http://localhost:8000` (if enabled)

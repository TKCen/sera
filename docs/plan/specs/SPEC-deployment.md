# SPEC: Deployment

> **Status:** DRAFT  
> **Source:** PRD §15  
> **Priority:** Phase 0 (Tier 1 support), Phase 2 (Tier 2), Phase 4 (Tier 3)  

---

## 1. Overview

SERA supports a **deployment spectrum** from single-entrypoint local development to multi-node enterprise clusters. The architecture is designed so that every component works at every tier — the difference is in backends, auth complexity, and operational tooling.

---

## 2. Deployment Tiers

### Tier 1: Local Development

| Aspect | Choice |
|---|---|
| **Entrypoint** | Single process (`sera start`) |
| **Database** | SQLite (runtime state) |
| **Memory** | File-based (persistence) |
| **Queue** | SQLite-backed |
| **Cache** | In-memory |
| **Auth** | Autonomous mode (no auth or auto-generated admin) |
| **Secrets** | Environment variables |
| **Connectors** | Built-in (in-process) |
| **Model providers** | gRPC to local provider (LM Studio, Ollama) |
| **Startup command** | `sera start` |

**Design note:** Tier 1 is a **single entrypoint**, not necessarily a single binary. Built-in connectors and providers run in-process; external ones connect via gRPC. The model provider communicates via gRPC even locally (uniform interface — see D2).

**Persistence model:** Database (SQLite) is used for **runtime state** (sessions, queue, audit). Files are used for **durable persistence** (memory, config, workspaces).

### Tier 2: Team / Private

| Aspect | Choice |
|---|---|
| **Deployment** | Docker Compose or single server |
| **Database** | PostgreSQL |
| **Memory** | File-based + git, or PostgreSQL |
| **Queue** | PostgreSQL-backed |
| **Cache** | Redis |
| **Auth** | JWT auth, basic RBAC |
| **Secrets** | File-based (encrypted) |
| **Connectors** | Built-in + external (gRPC) |
| **Model providers** | gRPC to remote/local providers |

### Tier 3: Enterprise

| Aspect | Choice |
|---|---|
| **Deployment** | Kubernetes / Nomad |
| **Database** | PostgreSQL HA |
| **Memory** | File-based + git, PostgreSQL, or LCM |
| **Queue** | TBD (Redis Streams / NATS — decision deferred) |
| **Cache** | Redis Cluster |
| **Auth** | OIDC + AuthZen + SSF CAEP/RISC |
| **Secrets** | Vault / AWS SM / Azure KV / GCP SM |
| **Connectors** | External (gRPC, independently scaled) |
| **HA** | Multi-node cluster with leader election |

---

## 3. Single Entrypoint Architecture (Tier 1)

```
┌──────────────────────────────────────────┐
│  sera start                               │
│  ┌──────────────────────────────────────┐ │
│  │  sera-gateway (in-process)           │ │
│  │  ├── HTTP/WS server                  │ │
│  │  ├── gRPC server                     │ │
│  │  ├── Built-in connectors (Discord)   │ │
│  │  ├── SQLite (sessions, queue, audit) │ │
│  │  ├── File memory                     │ │
│  │  └── In-memory cache                 │ │
│  └──────────────────────────────────────┘ │
│                    ↕ gRPC                  │
│  ┌──────────────────────────────────────┐ │
│  │  External adapters (if any)          │ │
│  │  ├── Custom connectors               │ │
│  │  ├── External tools                  │ │
│  │  └── External runtimes               │ │
│  └──────────────────────────────────────┘ │
└──────────────────────────────────────────┘
```

---

## 4. Container Architecture (Tier 2/3)

```yaml
# docker-compose.yml (Tier 2 example)
services:
  sera:
    image: sera:latest
    ports:
      - "8080:8080"    # HTTP/WS
      - "50051:50051"  # gRPC
    environment:
      SERA_DB_URL: "postgres://sera:password@db:5432/sera"
      SERA_REDIS_URL: "redis://redis:6379"
    depends_on:
      - db
      - redis

  db:
    image: postgres:17
    volumes:
      - pgdata:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine

  discord-connector:
    image: sera-connector-discord:latest
    environment:
      SERA_GATEWAY_URL: "sera:50051"
      SERA_SECRET_CONNECTORS__DISCORD_MAIN__TOKEN: "${DISCORD_TOKEN}"
```

---

## 5. Configuration by Tier

```yaml
sera:
  instance:
    tier: "local"       # local | team | enterprise
```

The `tier` setting controls which defaults are applied:

| Setting | `local` | `team` | `enterprise` |
|---|---|---|---|
| Auth mode | autonomous | jwt | oidc |
| Secret provider | env | file | vault / cloud |
| DB backend | sqlite | postgres | postgres-ha |
| Queue backend | sqlite | postgres | TBD |
| Cache backend | memory | redis | redis-cluster |
| Audit log | sqlite | postgres | postgres-ha |

Tier is a **default preset** — individual settings can always be overridden.

---

## 6. Health & Readiness

All deployment tiers expose:
- `/health` — liveness (is the process running?)
- `/ready` — readiness (are all subsystems initialized?)

Kubernetes deployments use these for pod lifecycle management.

---

## 7. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | The deployable unit |
| `sera-config` | [SPEC-config](SPEC-config.md) | Tier presets |
| `sera-db` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Database backend per tier |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Auth complexity per tier |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret provider per tier |
| Security model | [SPEC-security](SPEC-security.md) | Trust boundaries |

---

## 8. Open Questions

1. **Multi-node queue** — Redis Streams vs. NATS for Tier 3 queue backend? (Deferred to Phase 4)
2. **Leader election** — What mechanism for multi-node leader election? etcd? Raft built-in?
3. **Horizontal scaling** — How does session affinity work across multiple gateway nodes?
4. **Data migration** — How do you migrate from SQLite (Tier 1) to PostgreSQL (Tier 2)?
5. **Container registry** — Where are SERA container images published?
6. **Helm chart / deployment manifests** — Are these in-repo or separate?

---

## 9. Success Criteria

| Metric | Target |
|---|---|
| Local startup time | < 2 seconds (Tier 1) |
| Bootstrap time | < 5 minutes from `sera init` to first agent conversation |

# SPEC: Deployment

> **Status:** DRAFT
> **Source:** PRD §15, plus deltas from [SPEC-dependencies](SPEC-dependencies.md) §8.3 (`apalis` 0.7 as the canonical queue backend across all tiers — replaces hand-rolled `sera-queue`), §10.8 (NemoClaw platform compatibility matrix; pinned image digest with dual-field lockstep), §10.18 (**NVIDIA OpenShell K3s-in-Docker runtime substrate as a Tier-3 option**; operator-signed offline key storage), [SPEC-self-evolution](SPEC-self-evolution.md) §5.5, §10 (two-generation boot pattern; shadow workspace build environment; operator offline key storage)
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
| **Deployment** | Kubernetes / Nomad; **OpenShell K3s-in-Docker** as alternative sandbox backend |
| **Database** | PostgreSQL HA (or Dolt SQL server mode for multi-writer task graphs — see SPEC-workflow-engine §6.1) |
| **Memory** | File-based + git, PostgreSQL, or LCM |
| **Queue** | `apalis` 0.7 with Postgres/Redis backend — see SPEC-dependencies §8.3 (replaces the original "TBD Redis Streams/NATS" placeholder) |
| **Cache** | Redis Cluster or Dragonfly |
| **Auth** | OIDC + AuthZen + SSF CAEP/RISC |
| **Secrets** | Vault / AWS SM / Azure KV / GCP SM |
| **Connectors** | External (gRPC, independently scaled) |
| **HA** | Multi-node cluster with leader election; two-generation live (§3.1) for zero-downtime self-evolution |
| **Sandbox backend** | Native SERA (bollard + wasmtime) OR `OpenShellSandboxProvider` gRPC backend — see SPEC-tools §6a.5 |
| **Constitutional signer key** | Operator HSM or air-gapped key store (see §3.2) |

---

### 2a. Platform Compatibility Matrix

> **Source:** [SPEC-dependencies](SPEC-dependencies.md) §10.8 NemoClaw.

| Platform | Container Runtime | Status | Notes |
|---|---|---|---|
| Linux | Docker | **Tested (primary)** | Primary tested path for all tiers |
| Linux | containerd | Untested | Likely works; not in the tested path |
| Linux | Podman | Untested | rootless podman + socket path differs from docker; revisit |
| macOS (Apple Silicon) | Colima, Docker Desktop | Tested with limitations | Install Xcode Command Line Tools (`xcode-select --install`); start the runtime before `sera init` |
| macOS (Intel) | Docker Desktop | Untested | Likely works via x86_64 images |
| Windows WSL2 | Docker Desktop (WSL backend) | Tested with limitations | Requires WSL2 with Docker Desktop backend |
| DGX Spark / NVIDIA DGX | Docker | Tested | Use the standard installer; GPU passthrough via CDI or `--gpus all` |

CI runs on the first row. Other rows are smoke-tested ad-hoc; regressions there are accepted but not release-blocking until explicit user demand.

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

### 3.1 Two-Generation Boot Pattern (Tier 2/3 for Self-Evolution)

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §10.

For Tier-3 self-evolution, SERA supports running **two generations of the gateway+harness in parallel** — the running generation (N) and a new generation (N+1) promoted from a Change Artifact. Sessions are bound to a generation at creation and do not migrate mid-turn.

```
┌──────────────────────────────────────────────────┐
│  Front-Door Router (nginx / haproxy / envoy)      │
│  Routes by session.generation_marker              │
└───────────┬──────────────────────┬───────────────┘
            ↓                      ↓
┌───────────────────┐    ┌────────────────────┐
│  sera-gateway (N) │    │  sera-gateway (N+1) │
│  BuildIdentity:   │    │  BuildIdentity:      │
│    v0.3.0         │    │    v0.4.0            │
│  signer: key-A    │    │  signer: key-A       │
│  Active sessions  │    │  Canary sessions     │
└───────────────────┘    └────────────────────┘
            ↓                      ↓
            └──────────┬───────────┘
                       ↓
            ┌──────────────────────┐
            │  Shared Persistence  │
            │  (Postgres/Dolt/etc) │
            └──────────────────────┘
```

**Constraints:**

1. **Session-level routing only** — a session binds to N or N+1 at creation; the front-door router uses `generation_marker` from the session key to direct traffic. A session does not split across generations mid-turn (closes "live-migration replay corruption", SPEC-self-evolution §14.8).
2. **Shared persistence.** Both generations read/write the same Postgres/Dolt store. Schema must be forward-compatible between N and N+1 (see SPEC-versioning §4.7 reversibility contract).
3. **Canary workload gate.** N+1 must pass a canary workload (see SPEC-self-evolution §10.2) before new sessions are routed to it. If the canary fails, N+1 is killed; N continues untouched.
4. **Drain-then-retire.** Once N+1 is promoted as primary, N drains (default 5 min grace) before retirement. A rollback pointer is preserved for the change's rollback window.
5. **In-process mode exception.** For Tier-1 in-process deployments, two-generation is achieved by spawning a second subprocess with `AppServerTransport::Stdio` — no load balancer needed. See SPEC-gateway §7a.1.

### 3.2 Operator Offline Key Storage

> **Source:** [SPEC-self-evolution](SPEC-self-evolution.md) §9 (four meta-change scopes requiring offline key), §19 Q4 (key distribution open question).

The four most dangerous meta-change scopes (`ConstitutionalRuleSet`, `KillSwitchProtocol`, `AuditLogBackend`, `SelfEvolutionPipeline`) require a signature from a key that is **never stored in the running SERA instance**. Operators choose one of the following storage mechanisms depending on tier:

| Tier | Offline Key Storage |
|---|---|
| Tier 1 | File at `~/.sera/offline-key.age` (age-encrypted); not a production pattern — operator is the only principal anyway |
| Tier 2 | Hardware security key (YubiKey / Solo / Nitrokey) or air-gapped USB with age encryption |
| Tier 3 | HSM (Yubico HSM 2, AWS CloudHSM, Azure Dedicated HSM) with PKCS#11 interface; or Shamir secret sharing across multiple operators for high-value deployments |

**Boot verification.** The signer fingerprint is baked into the gateway binary via `BuildIdentity` (see SPEC-versioning §4.6). At boot, the gateway loads the trusted signer set from an OS-protected file (not in the normal config surface) and verifies that its `signer_fingerprint` is in the set. If not, the gateway refuses to start.

**Key rotation.** Rotation is itself a `ConstitutionalRuleSet` scope change — it requires a signature from the outgoing key. If the outgoing key is lost, recovery requires a full `sera init --force-reset` on each node, which wipes all existing sessions and durable state. There is no recovery path that preserves state without the old key — this is deliberate, to prevent an attacker from forging a rotation.

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
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | The deployable unit; two-generation transport |
| `sera-config` | [SPEC-config](SPEC-config.md) | Tier presets |
| `sera-db` | [SPEC-crate-decomposition](SPEC-crate-decomposition.md) | Database backend per tier |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Auth complexity per tier |
| `sera-secrets` | [SPEC-secrets](SPEC-secrets.md) | Secret provider per tier |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | OpenShell sandbox backend as Tier-3 alternative (§6a.5) |
| `sera-meta` | [SPEC-self-evolution](SPEC-self-evolution.md) | Two-generation boot pattern (§3.1); operator offline key storage (§3.2); constitutional signer binding at boot |
| Security model | [SPEC-security](SPEC-security.md) | Trust boundaries |
| Dependencies | [SPEC-dependencies](SPEC-dependencies.md) | §8.3 `apalis` queue resolution; §10.8 NemoClaw platform matrix + pinned image lockstep; §10.18 OpenShell K3s-in-Docker Tier-3 option |

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

# Epic 20: Egress Proxy

## Overview

Agent containers currently have binary network access — either full internet (`bridge`), internal-only (`agent_net`), or no network (`none`). The `network-allowlist` and `network-denylist` fields in sandbox boundaries and capability policies are resolved by `CapabilityResolver` but never enforced at the network level. An agent on `agent_net` or `bridge` can reach any host regardless of its allowlist.

This epic adds a Squid forward proxy on `agent_net` that enforces per-agent domain allowlists/denylists via SNI-based HTTPS filtering (no TLS MITM), with audit logging, egress metering, and bandwidth rate limiting.

## Context

- See `docs/ARCHITECTURE.md` → Docker Sandbox Model → Network isolation
- `SandboxManager` translates `network.outbound` to Docker network modes but specific-host filtering is unenforced
- `CapabilityResolver` fully resolves `network-allowlist` and `network-denylist` including `$ref` NamedList expansion
- `AuditService` provides a hash-chained immutable event log
- `MeteringService` tracks LLM tokens in a dual-table model (lightweight budget queries + full audit)
- The `web-fetch` built-in skill runs inside sera-core with basic IP filtering but no proxy awareness
- Agent containers use `LLMClient` (axios-based) with no `HTTP_PROXY` support

## Dependencies

- Epic 01 (Infrastructure Foundation) — Docker Compose, `agent_net` network
- Epic 03 (Docker Sandbox) — SandboxManager, capability resolution, container env injection
- Epic 04 (LLM Proxy) — MeteringService pattern
- Epic 11 (Scheduling & Audit) — AuditService, event schemas

---

## Stories

### Story 20.1: Squid proxy container and Docker Compose integration

**As** an operator
**I want** a Squid forward proxy running on `agent_net`
**So that** all agent egress traffic can be centrally controlled and monitored

**Acceptance Criteria:**
- [ ] `sera-egress-proxy` service added to `docker-compose.yaml` using `ubuntu/squid:latest` image
- [ ] Container connected to both `sera_net` (for sera-core communication) and `agent_net` (for agent traffic)
- [ ] Squid config mounted from `egress-proxy/squid.conf`
- [ ] ACL directory mounted at `/etc/squid/acls/` from host volume (sera-core writes agent ACL files here)
- [ ] Squid listens on port 3128 (standard proxy port)
- [ ] SNI-based HTTPS filtering via `ssl_bump peek` + `ssl_bump splice` — no TLS MITM, no CA cert required
- [ ] Plain HTTP filtered via standard `http_access` ACLs
- [ ] Default deny — no traffic passes without an explicit ACL entry
- [ ] Access log written to shared volume in structured JSON format (`logformat`)
- [ ] Health check: `squid -k check` or TCP check on port 3128
- [ ] Container labelled `sera.proxy: egress` for identification
- [ ] `egress_acls` and `egress_logs` named volumes defined in docker-compose

**Technical Notes:**
- SNI peek does not require generating a CA cert. Squid peeks at the TLS ClientHello SNI field and decides allow/deny based on the domain. The TLS session is then spliced (passed through) without decryption.
- JSON log format for structured parsing:
  ```
  logformat squid-json {"timestamp":"%{%Y-%m-%dT%H:%M:%S%z}tl","src_ip":"%>a","method":"%rm","url":"%ru","domain":"%>rd","status":%>Hs,"bytes":%<st,"duration_ms":%<tt}
  ```
- The dev compose (`docker-compose.dev.yaml`) should mirror the proxy service.

---

### Story 20.2: SandboxManager to proxy ACL generation

**As** sera-core
**I want** to generate per-agent Squid ACL files when agents are spawned or stopped
**So that** the proxy enforces each agent's resolved `network.outbound` allowlist

**Acceptance Criteria:**
- [ ] New `EgressAclManager` class in `core/src/sandbox/EgressAclManager.ts`
- [ ] On agent spawn, after `container.inspect()` returns the container IP: write an ACL file mapping that IP to the agent's resolved `network-allowlist` and `network-denylist`
- [ ] ACL file path: one file per agent in the `egress_acls` volume
- [ ] Master include file regenerated on every spawn/teardown — lists all active agent ACL files via `include`
- [ ] Squid reloaded after ACL changes via `squid -k reconfigure` (dockerode exec)
- [ ] On agent teardown: ACL file removed, master include regenerated, Squid reconfigured
- [ ] `SandboxManager` calls `EgressAclManager.onSpawn(instanceId, containerIp, resolvedCapabilities)` and `EgressAclManager.onTeardown(instanceId)`
- [ ] Wildcard `*` in network-allowlist generates an "allow all" ACL for that agent
- [ ] Empty outbound with `networkMode: 'none'` — no ACL file generated (container has no network)
- [ ] Container IP obtained from `container.inspect()` and stored in SandboxManager's in-memory container map
- [ ] sera-core's own IP gets a blanket allow ACL (for `web-fetch` skill requests)
- [ ] Unit tests: ACL generation for wildcard, specific hosts, and no-network cases

**Technical Notes:**
- Container IP from `container.inspect()` → `NetworkSettings.Networks.agent_net.IPAddress`
- Example ACL for an agent allowed `github.com` and `api.openai.com`:
  ```
  acl agent_10.0.1.5 src 10.0.1.5/32
  acl agent_10.0.1.5_domains dstdomain github.com api.openai.com
  http_access allow agent_10.0.1.5 agent_10.0.1.5_domains
  http_access deny agent_10.0.1.5
  ```

---

### Story 20.3: HTTP_PROXY injection into agent containers

**As** sera-core
**I want** agent containers to have `HTTP_PROXY` and `HTTPS_PROXY` environment variables set at spawn time
**So that** all outbound HTTP traffic from agent containers routes through the egress proxy

**Acceptance Criteria:**
- [ ] `SandboxManager.spawn()` injects `HTTP_PROXY=http://sera-egress-proxy:3128` and `HTTPS_PROXY=http://sera-egress-proxy:3128` when the agent has outbound network access
- [ ] `NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1` injected so internal SERA traffic bypasses the proxy
- [ ] `networkMode: 'none'` agents — no proxy vars injected (no network at all)
- [ ] **Key change:** `outbound: ['*']` agents now use `agent_net` instead of `bridge` — all outbound traffic goes through the proxy for audit/metering
- [ ] `SandboxInfo` type extended with `proxyEnabled: boolean` and `containerIp: string | null`
- [ ] Proxy env vars only injected when `EGRESS_PROXY_URL` env var is set on sera-core (graceful degradation if proxy not deployed)

**Technical Notes:**
- axios (used by agent-runtime's `LLMClient`) respects `HTTP_PROXY`/`HTTPS_PROXY` natively via `follow-redirects`
- The `NO_PROXY` list must include `sera-core` and `centrifugo` to keep internal traffic direct
- Eliminating `bridge` mode is the key architectural change — the proxy becomes the single exit point for all outbound traffic

---

### Story 20.4: Audit integration — access log to AuditService

**As** an operator
**I want** all agent egress requests logged in the immutable audit trail
**So that** I have a complete record of what external resources each agent accessed

**Acceptance Criteria:**
- [ ] New `EgressLogWatcher` class in `core/src/sandbox/EgressLogWatcher.ts`
- [ ] Tails the Squid JSON access log file using `fs.watch` + `readline`
- [ ] Parses each log line and maps `src_ip` back to `agentId` using SandboxManager's IP-to-agent mapping
- [ ] Creates `AuditEntry` for each egress request:
  - `actorType: 'agent'`
  - `actorId`: resolved agent instance ID
  - `eventType: 'network.egress'`
  - `payload: { domain, url, method, status, bytes, durationMs }`
- [ ] Denied requests (status 403) logged as `eventType: 'network.egress.denied'`
- [ ] Batching: buffer entries and flush to AuditService every 5 seconds or 50 entries (whichever first) to reduce write overhead
- [ ] Unknown source IPs logged as warnings but not written to audit trail
- [ ] Graceful shutdown: flush pending buffer on SIGTERM
- [ ] Log watcher started in `core/src/index.ts` startup sequence after SandboxManager initialization

**Technical Notes:**
- Access log shared via `egress_logs` Docker volume mounted into both `sera-egress-proxy` and `sera-core`
- `fs.watch` on the log file triggers readline to process new lines
- New audit event schema `NetworkEgressSchema` registered alongside existing schemas

---

### Story 20.5: Egress metering — bytes and request tracking

**As** an operator
**I want** per-agent egress bytes and request counts tracked alongside token usage
**So that** I can monitor and budget network consumption

**Acceptance Criteria:**
- [ ] New `egress_usage` table (see DB Schema below)
- [ ] `MeteringService` extended with `recordEgress(agentId, domain, bytes)` method
- [ ] Egress records written from the `EgressLogWatcher` batching loop alongside audit entries
- [ ] `GET /api/metering/egress?agentId=&from=&to=&groupBy=hour|day` — aggregated egress stats
- [ ] `GET /api/agents/:id/egress` — per-agent egress summary (total bytes, top domains, request count)
- [ ] Follow MeteringService's existing dual-write pattern

**Technical Notes:**
- Aggregation query pattern identical to existing `getAggregatedUsage` in MeteringService
- `maxEgressBytesPerHour` can be added to the capability model later for budget enforcement (schema-only in this story)

---

### Story 20.6: Rate limiting via Squid delay_pools

**As** an operator
**I want** per-agent bandwidth rate limits enforced at the proxy
**So that** a runaway agent cannot saturate the network

**Acceptance Criteria:**
- [ ] `network.maxBandwidthKbps` added to the capability model (resolvable via SandboxBoundary → CapabilityPolicy → manifest override)
- [ ] `EgressAclManager` includes `delay_pools` configuration in per-agent ACL files when `maxBandwidthKbps` is set
- [ ] Squid delay pool class 3 (per-source-IP limiting) used
- [ ] Default: no rate limit — only active when explicitly configured in boundary/policy
- [ ] Rate limit changes take effect on Squid reconfigure (no agent restart needed)
- [ ] `tier-2.yaml` default: `network.maxBandwidthKbps: 10240` (10 Mbps)
- [ ] Rate limit visible in `GET /api/agents/:id` response

**Technical Notes:**
- Squid `delay_pools` config for per-IP limiting:
  ```
  delay_pools 1
  delay_class 1 3
  delay_access 1 allow agent_10.0.1.5
  delay_parameters 1 -1/-1 -1/-1 1280000/1280000
  ```
  (1,280,000 bytes/sec ≈ 10 Mbps)

---

### Story 20.7: web-fetch skill migration to proxy

**As** sera-core
**I want** the `web-fetch` built-in skill to route through the egress proxy
**So that** web-fetch respects the same domain filtering and audit trail as direct agent HTTP calls

**Acceptance Criteria:**
- [ ] `web-fetch` handler uses the `EGRESS_PROXY_URL` env var when set to route requests through the proxy
- [ ] Inline private-IP regex filter removed — the proxy handles domain/IP filtering
- [ ] web-fetch checks the requesting agent's resolved `network-allowlist` via `CapabilityResolver` before making the request (since sera-core has a blanket allow at the proxy, the agent's policy must be enforced in code)
- [ ] If proxy is unavailable, web-fetch returns `{ success: false, error: 'Egress proxy unavailable' }` — no silent fallback to direct connections
- [ ] Backward compatibility: if `EGRESS_PROXY_URL` is not set, web-fetch continues using direct connections with the existing IP filter

**Technical Notes:**
- axios supports proxy directly: `axios.get(url, { proxy: { host, port } })`
- The `_context` parameter in the skill handler already carries agent identity — use it to look up resolved capabilities
- Two-layer check: (1) capability resolver validates the domain is in the agent's allowlist, (2) proxy enforces at network level as defense-in-depth

---

### Story 20.8: Dashboard UI for egress monitoring

**As** an operator
**I want** to see agent network egress activity in the web dashboard
**So that** I can monitor what agents are accessing and catch anomalies

**Acceptance Criteria:**
- [ ] New "Network" tab on the Agent Detail page
- [ ] Shows: total egress bytes (today), request count, top 10 domains by request count, top 10 domains by bytes
- [ ] Time-series chart of egress bytes over past 24h (reuse existing chart components)
- [ ] Table of recent egress requests (last 50): timestamp, domain, method, status, bytes, duration
- [ ] Denied requests highlighted visually
- [ ] Time range filter (1h, 6h, 24h, 7d)
- [ ] System-wide egress overview on the Observability dashboard: total egress by agent (bar chart), top domains across all agents
- [ ] API calls: `GET /api/agents/:id/egress` and `GET /api/metering/egress`

**Technical Notes:**
- Follow patterns from Epic 14 (sera-web Observability) for chart components and data fetching
- Use TanStack Query for API client hooks (existing pattern in `web/`)
- This story depends on Epic 14 (Observability) for shared chart infrastructure

---

## DB Schema Changes

```sql
-- Story 20.5: Egress usage tracking
CREATE TABLE egress_usage (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_id TEXT NOT NULL,
  domain TEXT NOT NULL,
  request_count INTEGER NOT NULL DEFAULT 1,
  bytes_out BIGINT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX egress_usage_agent_time_idx ON egress_usage (agent_id, created_at);
CREATE INDEX egress_usage_domain_idx ON egress_usage (domain, created_at);
```

No changes to existing tables. The `agent_instances` table already stores `resolved_capabilities` (JSONB) which will contain network capability data including `maxBandwidthKbps`. The `containerIp` field is in-memory only (in SandboxManager's container map).

---

## New capability dimension

```yaml
network:
  outbound:
    allow: ["github.com", "$ref:npm-registry"]
    deny: ["$ref:always-denied-hosts"]
  maxBandwidthKbps: 10240    # new — Story 20.6
```

Resolved through the standard three-layer model: `SandboxBoundary ∩ CapabilityPolicy ∩ ManifestOverride`. The proxy enforces the resolved `allow`/`deny` lists; `maxBandwidthKbps` maps to Squid delay pools.

---

## Architecture diagram

```
┌──────────────────────────────────────────────────────────────────┐
│  sera-core                                                       │
│  ┌──────────────┐  ┌────────────────┐  ┌──────────────────────┐ │
│  │ SandboxMgr   │  │ EgressAclMgr   │  │ EgressLogWatcher     │ │
│  │ (spawn/stop) │──│ (write ACLs)   │  │ (tail → Audit/Meter) │ │
│  └──────┬───────┘  └───────┬────────┘  └──────────┬───────────┘ │
│         │                  │ ACL files             │ JSON log    │
│         │                  ▼                       ▼             │
│         │         ┌────────────────────────────────────┐        │
│         │         │       egress_acls / egress_logs    │        │
│         │         │          (shared volumes)          │        │
│         │         └────────────────┬───────────────────┘        │
└─────────┼──────────────────────────┼────────────────────────────┘
          │                          │
          │ spawn container          │ reads ACLs / writes logs
          ▼                          ▼
┌─────────────────┐    ┌──────────────────────────┐
│  Agent Container │    │  sera-egress-proxy       │
│  HTTP_PROXY ─────┼───►│  (Squid on agent_net)    │
│  HTTPS_PROXY     │    │  SNI peek → allow/deny   │
│                  │    │  delay_pools → rate limit │
└─────────────────┘    └───────────┬──────────────┘
                                   │ allowed traffic
                                   ▼
                              Internet
```

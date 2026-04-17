# Dynamic Model & Provider Routing with Health-Aware Selection

**Status:** Design — research & design pass for bead `sera-48j`.
**Owner:** SERA gateway / `sera-models` maintainers.
**Related specs:** `docs/plan/specs/SPEC-gateway.md` §12 (Provider routing / failover), `docs/plan/ARCHITECTURE-2.0.md` §1 (Gateway as LLM proxy).
**Related beads:** `sera-routing-phase1`, `sera-routing-phase2`, `sera-routing-phase3` (rollout phases — see §13).

---

## 1. Problem statement

Today SERA's gateway proxies LLM traffic through `rust/crates/sera-gateway/src/routes/llm_proxy.rs`. Provider selection is **static**: `chat_completions()` looks up the upstream by exact `model_name` match in `state.providers` (`sera_config::providers::ProviderEntry`), grabs the first hit, and forwards the request. Failure means a user-visible 5xx or a pass-through of the upstream error — there is no retry, no fallback to a sibling model, and no awareness of provider health.

A partial mitigation exists in `rust/crates/sera-gateway/src/services/llm_router.rs`: `LlmRouter` implements priority-ordered failover with a simple per-provider circuit breaker (`ProviderHealth { failures, last_failure, circuit_open }`). However:

- It is not wired into `llm_proxy::chat_completions()` — the public proxy path bypasses it entirely.
- Selection is priority-sorted only; latency, error rate, cost, and recency are not considered.
- Model discovery is static — providers are read once from `providers.json` into `ProvidersConfig`.
- `ModelProvider::health_check()` in `rust/crates/sera-models/src/provider.rs` has a default `Ok(())` impl and is never polled.
- The runtime-side `sera-runtime/src/llm_client.rs` has its own `LlmError` retry classification, duplicating logic.

This leaves SERA exposed to hard outages when a single upstream degrades, and it prevents operators from steering traffic by cost or latency SLOs.

## 2. Goals / Non-goals

### Goals

1. Introduce a **`RoutingPolicy` trait** in `sera-models` so selection strategies are swappable.
2. Add a **`HealthStore` component** that records observed latency, error rate, rate-limit hits, and cost per `(provider, model)` pair.
3. Replace the static lookup in `llm_proxy::chat_completions()` with a policy-driven pick that can fall back on failure.
4. Support **weighted, health-aware selection** (latency + error-rate + cost + recency) with config-driven weights.
5. Make **auto-discovery** of provider model catalogs periodic so newly-deployed models become routable without a gateway restart.
6. Preserve existing behaviour when routing is disabled (rollback lever).

### Non-goals

- **Global cross-region load balancing.** Out of scope; the initial scope is intra-gateway.
- **Per-request ML-based scheduling.** Weighted scoring is intentionally simple and explainable.
- **Replacing `sera-config::providers::ProvidersConfig` as the on-disk truth.** Routing reads from it; edits stay with `PATCH /api/providers/:modelName`.
- **Streaming retry.** If a streaming response fails mid-stream, we do not re-run against a different provider in phase 1–3; the client must retry. Tradeoff: mid-stream retry requires replaying already-emitted SSE tokens, which is semantically lossy.

## 3. Current state (concrete module references)

| Concern                            | Module / symbol                                                                |
| ---------------------------------- | ------------------------------------------------------------------------------ |
| Provider abstraction               | `sera_models::provider::ModelProvider` (`rust/crates/sera-models/src/provider.rs`) |
| Provider config enum               | `sera_models::provider::ProviderConfig`                                         |
| Error classification               | `sera_models::error::ModelError` (`RateLimit`, `Timeout`, `NotAvailable`, …)   |
| Static provider registry           | `sera_config::providers::ProvidersConfig` (`rust/crates/sera-config/src/providers.rs`) |
| Gateway proxy (public path)        | `sera_gateway::routes::llm_proxy::chat_completions` + `list_models`            |
| Admin CRUD for providers           | `sera_gateway::routes::providers::{list_providers, add_provider, update_provider, delete_provider}` |
| Unused failover scaffolding        | `sera_gateway::services::llm_router::LlmRouter` (+ `ProviderHealth`)           |
| Runtime-side retry classification  | `sera_runtime::llm_client::LlmError`                                           |
| Dynamic provider manager           | `sera_gateway::services::dynamic_provider_manager`                             |

The **duplication** between `sera-models::ProviderConfig` and the ad-hoc `sera-gateway::services::llm_router::ProviderConfig` is the first thing the new design unifies. Tradeoff: forcing everyone onto one trait imposes a refactor cost on the `llm_router` service, but eliminates drift between two slightly different enums.

## 4. Proposed architecture — `RoutingPolicy` + `HealthStore`

We introduce two concepts and place them in a new module `sera_models::routing`:

- **`RoutingPolicy`** — pure selector. Given a candidate pool plus a `HealthSnapshot`, returns one or more `ProviderConfig`s in try-order. Stateless (all state is injected).
- **`HealthStore`** — rolling telemetry sink. Records one observation per terminal LLM call outcome and exposes cheap read snapshots.

```rust
/// A per-request selection context.
pub struct RoutingRequest<'a> {
    pub model_hint: Option<&'a str>,   // OpenAI-compat "model" field, or None
    pub tags: &'a [&'a str],           // agent-supplied routing tags, e.g. ["reasoning", "low-cost"]
    pub max_cost_usd_per_1k: Option<f64>,
    pub require_tool_calling: bool,
}

/// Snapshot view of observed provider health.
pub trait HealthStore: Send + Sync {
    fn snapshot(&self) -> HealthSnapshot;
    fn record_success(&self, key: &ProviderKey, latency_ms: u32, cost_usd: f64);
    fn record_failure(&self, key: &ProviderKey, err: &ModelError, latency_ms: u32);
}

/// Pluggable selection strategy.
#[async_trait]
pub trait RoutingPolicy: Send + Sync {
    /// Return an ordered list of candidates to try. The caller walks the list
    /// until one succeeds or the list is exhausted.
    async fn select(
        &self,
        req: &RoutingRequest<'_>,
        pool: &[ProviderCandidate],
        health: &HealthSnapshot,
    ) -> Result<Vec<ProviderCandidate>, RoutingError>;

    /// Stable name for metrics/logs (e.g. "weighted", "priority", "round_robin").
    fn name(&self) -> &'static str;
}
```

`ProviderKey` is a stable `(provider, model_name)` tuple used as the `HealthStore` index. `ProviderCandidate` wraps `ProviderConfig` + precomputed capability flags (context window, tool-call support, vision, reasoning) read from `ProviderEntry`.

**Tradeoff — trait lives in `sera-models` vs. `sera-gateway`:** placing it in `sera-models` keeps runtime-side code (`sera-runtime`) able to reuse the same policy when it makes direct calls, but it drags `HealthStore` out of its natural home in the gateway. We accept this because BYOH agents may also want local routing.

**Tradeoff — async trait vs. sync selector:** `select()` is async so implementations can consult stores that live behind `tokio::RwLock` or remote caches. The ergonomic cost of `#[async_trait]` is already paid elsewhere (see `ModelProvider`).

## 5. Data model — `ModelHealth`

```rust
/// One record per (provider, model). Thread-safe interior.
pub struct ModelHealth {
    pub p95_latency_ms: u32,       // rolling p95 over last N=100 or 10m window
    pub err_rate_10m: f32,         // failed / total over 10 min, [0.0, 1.0]
    pub last_429_at: Option<Instant>,
    pub last_ok_at: Option<Instant>,
    pub cost_per_1k_tokens: f64,   // seeded from config, refined on first usage event
    pub circuit: CircuitState,     // Closed | HalfOpen { probe_at } | Open { reopen_at }
    pub observations: u64,         // total terminal outcomes recorded
}
```

**Storage:** in-memory `DashMap<ProviderKey, ArcSwap<ModelHealth>>` inside `InMemoryHealthStore`, with a `snapshot()` that clones a `HashMap` under a single read pass. Tradeoff: `DashMap` offers shard-local locking (fast under contention) but produces less-consistent snapshots than a single `RwLock<HashMap>`. We accept the minor inconsistency because routing reads tolerate stale-by-milliseconds data.

**Rolling windows:** a fixed-size `VecDeque<LatencySample>` per key, eagerly trimmed on write. A Prometheus-backed variant (`PromHealthStore`) is deferred to phase 3 as an optional feature flag. Tradeoff: bounded memory (N=100 samples × ~24 bytes ≈ 2.4 KB per model; bounded by the provider count) vs. the historical granularity a TSDB gives.

## 6. Selection algorithm — weighted score

Given a candidate pool `P`, the `WeightedRoutingPolicy` computes for each `c ∈ P`:

```
score(c) = w_lat * norm_latency(c)
        + w_err * c.err_rate_10m
        + w_cost * norm_cost(c)
        - w_recency * recency_bonus(c)
```

- `norm_latency(c) = min(c.p95_latency_ms / target_p95_ms, 1.0)` — clamped so a single slow outlier cannot dominate the other signals. Tradeoff: clamping loses discrimination between "slow" and "catastrophically slow"; the `err_rate` term picks up the slack once failures start.
- `norm_cost(c) = min(c.cost_per_1k / budget_cost_per_1k, 1.0)` — cost is optional. If `budget_cost_per_1k` is unset, the term is zeroed.
- `recency_bonus(c)` is nonzero if `last_ok_at` is within `fresh_window_secs` (default 60 s) and `circuit == Closed`. This biases toward providers we know are warm.
- **Lower score is better.** Candidates are sorted ascending.

**Tie-breakers**, in order:

1. Lower `p95_latency_ms`.
2. Lower `cost_per_1k_tokens`.
3. Stable priority from `ProviderEntry` (preserves operator intent when telemetry is flat).
4. Deterministic hash of `ProviderKey` (prevents thundering-herd on identical-score twins).

**Hard filters applied before scoring:**

- `circuit == Open` → excluded until `reopen_at`.
- `last_429_at` within `rate_limit_cooldown_secs` (default 15 s) → excluded.
- `require_tool_calling` set but provider lacks the capability → excluded.
- `max_cost_usd_per_1k` exceeded → excluded.

Tradeoff: hard filters protect SLOs but can empty the pool. When the pool is empty the policy returns `RoutingError::NoCandidate`; the caller (gateway) decides whether to 503 or degrade to a bypass path.

## 7. Auto-discovery — periodic `list_models()` poll + cache invalidation

A background task `DiscoveryLoop` runs every `discovery_interval_secs` (default 600 s) and for each `ProviderEntry` with `dynamic_provider_id: Some(_)` calls the upstream's `/v1/models` (or equivalent). Results refresh an in-memory `CatalogCache`:

- **Additions** become new `ProviderCandidate`s; health for the new key starts at `ModelHealth::optimistic_defaults()` so traffic actually reaches them for calibration. Tradeoff: optimistic start briefly over-routes to unknown models; capped by a `discovery_grace_max_qps` sanity limit.
- **Removals** mark the candidate `disabled = true` but retain its `ModelHealth` for `stale_retention_secs` in case of a transient discovery error.
- **Errors** during polling do not purge — we keep the last-known catalog and emit a `routing.discovery.failed` metric.

Invalidation paths:

- `PATCH /api/providers/:modelName` pushes an event into a `tokio::sync::broadcast` channel consumed by the discovery loop, which re-polls that single provider immediately.
- A manual `POST /api/providers/refresh` endpoint (new) forces a full resync. Tradeoff: exposing a force-refresh endpoint adds an attack surface; gated behind the admin key middleware already on `routes::providers`.

## 8. Failover chain

`chat_completions()` asks the policy for `Vec<ProviderCandidate>` and walks it:

1. Attempt candidate `i`; record `record_success` or `record_failure` based on the outcome.
2. On failure, classify via `ModelError`:
   - `RateLimit`, `NotAvailable`, `Timeout`, `Http(...)` → **retriable** → try next candidate.
   - `Authentication`, `ContextLengthExceeded`, `InvalidResponse` → **non-retriable** → bubble up immediately.
3. Stop after `max_attempts` (default 3) or list exhaustion.

**Tradeoff — per-attempt budget vs. total deadline:** we cap total wall-clock at `request_deadline_ms` (default 60 s) so a long chain of slow providers cannot exceed a client timeout. A strict deadline risks cutting off a final candidate that would have succeeded; we prefer predictable latency.

**Streaming caveat:** if `stream: true`, failover is only available for pre-first-token errors. Once bytes have started flowing, a mid-stream failure is terminal. This is enforced by flipping a `streaming_started` bool before the first flush in the SSE path of `llm_proxy.rs`.

## 9. Integration points

- **`sera-gateway::routes::llm_proxy::chat_completions`** — replace the direct `providers.iter().find(…)` with `router.route(&RoutingRequest { … }).await?`. The existing budget gate (`MeteringRepository::check_budget`) stays in front of the router. Metering's `record_usage` now also feeds `HealthStore::record_success` with the measured `latency_ms` and resolved `cost_usd`.
- **`sera-gateway::services::llm_router::LlmRouter`** — repurposed into the default `HealthStore` + `WeightedRoutingPolicy` implementation. Existing `LlmRouter` API kept as a deprecated re-export for one minor version to avoid breaking `sera-runtime`. Tradeoff: cruft for a release, but zero callers churn in phase 1.
- **`sera-gateway::state::AppState`** — add `router: Arc<dyn RoutingPolicy>` and `health: Arc<dyn HealthStore>`.
- **Agent manifest** (`sera-types::manifest::AgentManifest`) — extend with an optional `routing` block:

  ```rust
  pub struct AgentRoutingSpec {
      pub policy: Option<String>,               // "weighted" | "priority" | "round_robin"
      pub tags: Vec<String>,                    // forwarded as RoutingRequest.tags
      pub max_cost_usd_per_1k: Option<f64>,
      pub require_tool_calling: bool,
  }
  ```

  The gateway reads `AgentRoutingSpec` off the agent's manifest during `chat_completions()` and uses it to build `RoutingRequest`. Tradeoff: manifest growth vs. inline per-request headers; manifest wins because policy decisions are infra concerns, not per-call.

## 10. Config — `RoutingConfig` + YAML example

A new top-level section in gateway config (`sera-config::core_config`):

```yaml
routing:
  enabled: true
  policy: weighted                    # weighted | priority | round_robin
  weights:
    latency: 0.5
    err_rate: 0.35
    cost: 0.10
    recency: 0.05
  target_p95_ms: 2500
  budget_cost_per_1k_usd: 0.010       # optional; omit to disable cost scoring
  rate_limit_cooldown_secs: 15
  fresh_window_secs: 60
  circuit_breaker:
    failure_threshold: 5
    reset_secs: 30
    half_open_probe_ratio: 0.10
  failover:
    max_attempts: 3
    request_deadline_ms: 60000
  discovery:
    enabled: true
    interval_secs: 600
    stale_retention_secs: 3600
  health_store:
    backend: memory                   # memory | prom (phase 3)
    sample_window: 100
```

Tradeoff — flat vs. nested: nested groups read well but require more serde boilerplate. We accept the boilerplate cost once, in exchange for the ability to split `routing.circuit_breaker` overrides into a separate YAML file via merge.

## 11. Metrics / observability

Prefix `sera_routing_*`:

| Metric                                        | Type      | Labels                      |
| --------------------------------------------- | --------- | --------------------------- |
| `sera_routing_requests_total`                 | counter   | `policy`, `outcome`         |
| `sera_routing_attempts_total`                 | counter   | `provider`, `model`, `result` (`ok`/`retry`/`final_fail`) |
| `sera_routing_candidate_pool_size`            | histogram | `policy`                    |
| `sera_routing_selection_duration_us`          | histogram | `policy`                    |
| `sera_routing_provider_latency_ms`            | histogram | `provider`, `model`         |
| `sera_routing_circuit_state`                  | gauge     | `provider`, `model`, `state` |
| `sera_routing_discovery_failures_total`       | counter   | `provider`                  |

Tracing span `routing.select` wraps each `RoutingPolicy::select` call with fields `{ policy, pool_size, winner, score }`. Tradeoff: a high-cardinality `model` label blows up Prometheus storage; we cap the label set via a hash-truncation when `len(models) > 50`.

## 12. Testing strategy

- **Mock provider.** `sera-testing::mock_provider::MockProvider` (new) implementing `ModelProvider` with injectable `latency_ms`, `fail_after_n`, `fail_with: ModelError`. Tradeoff: an injectable mock grows in scope easily — strictly limit it to the six knobs the policy tests need.
- **Unit tests** on `WeightedRoutingPolicy::select`: feed synthetic `HealthSnapshot`s and assert ordering (`w_cost=1.0` selects cheapest, `w_err=1.0` dodges the failing provider, ties break by `ProviderKey` hash).
- **Property tests** (`proptest`): for any non-empty pool with mixed health, `select` must return a non-empty result unless all circuits are open or all hard filters excluded everything.
- **Integration test** in `sera-gateway/tests/routing_failover.rs`: stand up three mock providers (one returns `RateLimit`, one returns `Timeout`, one returns `Ok`), verify that `chat_completions()` succeeds and that the observed call-order matches scoring.
- **Circuit-breaker test**: feed N failures, assert `circuit == Open`, advance mocked `Instant` by `reset_secs`, assert half-open probe proceeds and a single success closes the circuit. Tradeoff: `tokio::time::pause` only works in single-threaded runtimes; gate the test `#[tokio::test(flavor = "current_thread", start_paused = true)]`.
- **Contract test**: `ModelProvider::health_check()` real impls (OpenAI, Anthropic, Local) are invoked against a mock HTTP server to verify `record_failure` is called with the right `ModelError` variant.

## 13. Rollout plan

### Phase 1 — `HealthStore` + basic metrics (bead `sera-routing-phase1`)

- Add `sera_models::routing` module with `HealthStore`, `ModelHealth`, `InMemoryHealthStore`.
- Wire `llm_proxy::chat_completions` to *observe* into the store without changing selection (shadow mode). Tradeoff: observe-only ships faster but delays the operator-visible win to phase 2.
- Emit `sera_routing_provider_latency_ms` and `sera_routing_attempts_total`.
- Exit criteria: 7 days of production telemetry show non-zero samples for all configured providers; p95 numbers look plausible vs. external monitoring.

### Phase 2 — Weighted selection (bead `sera-routing-phase2`)

- Land `RoutingPolicy` trait and `WeightedRoutingPolicy`.
- Wire into `chat_completions`; keep the static path behind `routing.enabled: false` as a rollback.
- Add `AgentRoutingSpec` on manifests.
- Exit criteria: A/B flag flip on 10% of traffic for 48 h shows no regression in p95 or success rate; the weighted pick beats static priority on at least one metric (synthetic chaos test forcing one provider's latency up by 3×).

### Phase 3 — Circuit breakers + auto-discovery (bead `sera-routing-phase3`)

- Promote the half-open/open circuit state machine from scaffolding to the default.
- Add `DiscoveryLoop` and the `POST /api/providers/refresh` endpoint.
- Optional `PromHealthStore` backend under a cargo feature `routing-prom`.
- Exit criteria: a forced-kill chaos test (one provider returns 503 for 10 min) routes 100% of affected traffic to survivors within 30 s; newly-added models from upstream catalog become routable without restart.

**Tradeoff across phases:** the three-phase cut keeps each PR reviewable and each rollback isolated. A single big-bang PR would land faster but concentrates risk — we prefer reversibility.

## 14. Open questions

1. **Cost source of truth.** `cost_per_1k_tokens` is currently seeded from `ProviderEntry` (config-supplied). Should we derive it from provider pricing APIs (OpenAI publishes a machine-readable pricing list; Anthropic does not)? Leaning config-first, auto-refresh later.
2. **Per-agent overrides vs. per-request headers.** An `X-Sera-Routing-Tags` header could short-circuit the manifest lookup. Ops benefit (runtime steering) vs. security concern (agents hand-crafting routing hints). Needs owner input.
3. **Global throttling.** `sera-queue::GlobalThrottle` already exists. Should the routing policy consult it and pre-exclude providers whose global QPS is saturated, or let the throttle return 429s and learn organically?
4. **Backpressure into the session queue.** When all candidates are unhealthy, should we `Defer` the submission back to the SQ (retry on next tick) instead of returning 503? Interacts with SPEC-gateway §4 event model; unresolved.
5. **Half-open probe policy.** Send exactly one trial request, or ramp up via `half_open_probe_ratio`? Ramp is safer under high QPS but complicates the state machine. Decision deferred to phase 3 after we see real traffic shapes.
6. **BYOH agents.** They hit the gateway's `/v1/llm/*` surface today, but some may want local routing (trusted agents running in Tier 3 partitions). Does `sera-byoh-agent` embed `WeightedRoutingPolicy` directly, or always delegate? Recommend delegation until a concrete use case appears.
7. **Interaction with LiteLLM.** SERA currently points at a LiteLLM gateway for some providers. If LiteLLM is already doing routing, our layer becomes redundant. Document an explicit "LiteLLM front-ends a single upstream" deployment pattern to avoid double-routing. Tradeoff: two routers in series can cancel each other's strategies.
8. **Persistence of health data.** The `InMemoryHealthStore` loses all samples on gateway restart, forcing a cold-start re-calibration window. Should we persist the last-known `ModelHealth` snapshot to `sera-db` on shutdown (or every 60 s) and seed the store on boot? Tradeoff: persistence buys faster post-restart convergence at the cost of a DB write per shutdown and a race window where stale data biases the first post-boot picks. Default recommendation: do not persist; operators who need it can enable the `routing-prom` backend in phase 3, which is persistent by construction.
9. **Multi-tenant fairness.** If one agent is saturating a provider and driving its err rate up, other agents' requests will be routed away — even though the healthy provider from their perspective is "fine, just busy serving that one noisy neighbour." Do we partition `HealthStore` by tenant, by agent, or leave global? Leaning global for phase 1–3 because per-tenant partitioning multiplies memory by tenant count and complicates the selection algorithm; revisit once multi-tenant usage is material.

---

## Appendix A — Reference call sites

The following files are directly affected by the design and should be touched in the rollout PRs. Listed with the nature of the change, so reviewers have a checklist when the phases land.

| File                                                                    | Change kind                                                    |
| ----------------------------------------------------------------------- | -------------------------------------------------------------- |
| `rust/crates/sera-models/src/lib.rs`                                    | Add `pub mod routing;` re-export                               |
| `rust/crates/sera-models/src/routing.rs` (new)                          | Trait + `ModelHealth` + `InMemoryHealthStore`                  |
| `rust/crates/sera-gateway/src/state.rs`                                 | Extend `AppState` with `router` and `health` fields            |
| `rust/crates/sera-gateway/src/routes/llm_proxy.rs`                      | Replace static provider lookup with `router.route(…)`          |
| `rust/crates/sera-gateway/src/routes/providers.rs`                      | Emit catalog-change events on PATCH/POST/DELETE                |
| `rust/crates/sera-gateway/src/services/llm_router.rs`                   | Reshape into `WeightedRoutingPolicy` implementing the new trait |
| `rust/crates/sera-gateway/src/services/dynamic_provider_manager.rs`     | Own the `DiscoveryLoop` task spawn                             |
| `rust/crates/sera-config/src/core_config.rs`                            | Add `RoutingConfig` to the top-level config struct             |
| `rust/crates/sera-types/src/manifest.rs`                                | Add optional `AgentRoutingSpec`                                |
| `rust/crates/sera-testing/src/mock_provider.rs` (new)                   | `MockProvider` with injectable latency + error knobs           |
| `rust/crates/sera-gateway/tests/routing_failover.rs` (new)              | End-to-end failover integration test                           |

## Appendix B — Non-obvious interactions

- **Budget gate ordering.** The budget check in `llm_proxy::chat_completions` runs *before* routing. If we inverted that order, a budget-exceeded tenant could still drive health telemetry for providers they are not allowed to hit, polluting the store. Keeping budget-first is the correct contract.
- **Streaming `tool_calls`.** The `ToolCalls` `FinishReason` in `sera_models::response::FinishReason` is recorded as a success for health purposes — tool-call completion is not a provider failure, even though the agent's caller may still be doing work afterwards. Tradeoff: this slightly inflates the success signal compared to "end-to-end agent turn succeeded," but keeping provider-level and agent-level signals separate is the simpler contract.
- **Context-length errors.** `ModelError::ContextLengthExceeded` is classified as **non-retriable** in §8. This is deliberate: routing to a second model with the same context limit will fail identically. A future enhancement could route to a larger-context sibling when available; not in scope for phase 1–3 but called out here so the first reader of §8 is not surprised.

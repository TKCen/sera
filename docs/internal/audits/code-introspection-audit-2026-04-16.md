# SERA 2.0 Rust Workspace — Code Introspection & Quality Audit

**Date:** 2026-04-16
**Branch:** sera20
**Build:** PASS (cargo build --release)
**Tests:** PASS (1,437 tests, 0 failures, 3 ignored)
**Crates audited:** 27

---

## Executive Summary

| Metric | Value |
|--------|-------|
| Total LOC (src/) | ~68,000 |
| Total tests | 1,437 |
| Crates with 0 test modules | 4 (sera-tui, sera-telemetry, sera-byoh-agent, sera-models) |
| Issues found | ~130 |
| P1 (critical) | 18 |
| P2 (high) | 55 |
| P3 (medium/low) | ~57 |

---

## Top 10 Most Critical Issues

| # | Location | Category | Description |
|---|----------|----------|-------------|
| 1 | sera-auth/src/jwt.rs:61 | SECURITY/P1 | Hardcoded default JWT secret `"sera-secret"` in Default impl |
| 2 | sera-tools/src/sandbox/docker.rs:52-104 | STUB/P1 | All SandboxProvider methods are Phase 0 stubs returning fake data |
| 3 | sera-config/src/shadow_store.rs:62 | STUB/P1 | `unimplemented!()` blocks shadow deployment feature |
| 4 | sera-workflow/src/sleeptime.rs:287-446 | STUB/P1 | All 5 consolidation phases are POST-MVS no-op stubs |
| 5 | sera-workflow/src/coordination.rs:92-94 | BUG/P1 | `ConcurrencyPolicy::Parallel` silently falls back to Sequential |
| 6 | sera-gateway/src/services/notification_service.rs:180+ | BUG/P1 | `.expect()` on JSON parsing in PRODUCTION code — panics on bad input |
| 7 | sera-db (16 repository modules) | TEST/P1 | Zero test coverage on most DB repositories (agents, sessions, api_keys, audit, memory, secrets, etc.) |
| 8 | sera-gateway/src/routes/stubs.rs | INCOMPLETE/P1 | 40+ endpoint stubs returning placeholder data |
| 9 | sera-byoh-agent/src/health.rs:11-12 | BUG/P1 | `.unwrap()` on network binding — panics if port unavailable |
| 10 | sera-db dynamic SQL (agents.rs, audit.rs, schedules.rs) | SECURITY/P2 | Dynamic query construction via format! — fragile param_idx management |

---

## Issues by Category

### 1. Stubs & Incomplete Implementations (28 issues)

#### P1 — Blocks functionality
| Location | Description |
|----------|-------------|
| sera-config/src/shadow_store.rs:62 | `unimplemented!("commit_overlay")` — shadow deployments broken |
| sera-tools/src/sandbox/docker.rs:52-104 | All SandboxProvider methods return fake data |
| sera-tools/src/sandbox/wasm.rs:18-54 | All methods return `NotImplemented` |
| sera-tools/src/sandbox/microvm.rs:18-54 | All methods return `NotImplemented` |
| sera-tools/src/sandbox/external.rs:18-54 | All methods return `NotImplemented` |
| sera-tools/src/sandbox/openshell.rs:18-54 | All methods return `NotImplemented` |
| sera-workflow/src/sleeptime.rs:287-446 | 5 consolidation phases are POST-MVS no-ops |
| sera-workflow/src/sleeptime.rs:449 | `deduct_tokens()` is a no-op |
| sera-session/src/memory_wrapper.rs:449 | `deduct_tokens()` no-op (POST-MVS) |
| sera-skills/src/knowledge_lint.rs:419 | TODO(#312): Contradiction/KnowledgeGap checks stubbed |
| sera-gateway/src/session_persist.rs:77-105 | P0 stub — git2 integration deferred |
| sera-gateway/src/services/mcp_server_manager.rs:137,167 | Acknowledged stub implementations |

#### P2 — Placeholder logic
| Location | Description |
|----------|-------------|
| sera-auth/src/authz.rs:198-227 | DefaultAuthzProvider always returns Allow; role-based ACL TODO |
| sera-hooks/src/executor.rs:110 | TODO: handle updated_input from HookResult::Continue |
| sera-runtime/src/tools/dispatcher.rs:6 | TODO: migrate to TraitToolRegistry for policy enforcement |
| sera-gateway/src/routes/stubs.rs:1-739 | 40+ endpoint stubs by design |
| sera-gateway/src/routes/embedding.rs:470-543 | Multiple embedding endpoints return zero-vectors |
| sera-gateway/src/routes/chat.rs:771 | Hardcoded stub response string |
| sera-cache/src/lib.rs:56-59 | MokaBackend ignores TTL parameter |
| sera-cache/src/lib.rs | No Redis/Fred backend (Phase 1) |
| sera-tui/src/cli.rs:40 | Chat subcommand is placeholder |
| sera-testing/src/lib.rs | Promises DB pool setup, golden tests, contract runners — only has mocks |

### 2. Security Issues (12 issues)

#### P1
| Location | Description |
|----------|-------------|
| sera-auth/src/jwt.rs:61 | Hardcoded JWT secret `"sera-secret"` in Default impl |

#### P2
| Location | Description |
|----------|-------------|
| sera-config/src/core_config.rs:74-105 | Hardcoded dev secrets in defaults (sera_bootstrap_dev_123, sera-token-secret, etc.) |
| sera-tui/src/cli.rs:15 | Hardcoded default API key in CLI binary |
| sera-auth/src/middleware.rs:82-85 | Operator ID detection via "op-" prefix — fragile |
| sera-db/src/agents.rs:267-275 | Dynamic SQL via format! with param_idx |
| sera-db/src/audit.rs:76-94 | Dynamic SQL via format! with manual param tracking |
| sera-db/src/operator_requests.rs:42-58 | Dynamic SQL construction for filtering |
| sera-db/src/schedules.rs:135-149 | Complex dynamic query with manual param tracking |
| sera-db/src/secrets.rs:84-85 | UTF-8 decode failure on decryption silently maps to generic DbError |
| sera-runtime/src/turn.rs:344 | Steer injection — no validation/sanitization of content |
| sera-runtime/src/llm_client.rs:592-603 | Error classification via substring matching — brittle to API changes |

### 3. Bugs & Error Handling (22 issues)

#### P1 — Panics in production code
| Location | Description |
|----------|-------------|
| sera-gateway/src/services/notification_service.rs:180-207 | `.expect()` on JSON parsing in production |
| sera-gateway/src/services/memory_manager.rs:178-276 | `.expect()` on JSON/YAML parsing in production |
| sera-gateway/src/services/skill_registry.rs:112,279 | `.expect()` on YAML parsing |
| sera-byoh-agent/src/health.rs:11-12 | `.unwrap()` on TcpListener bind |
| sera-byoh-agent/src/main.rs:75-77 | `health_handle.abort()` without graceful shutdown |
| sera-workflow/src/coordination.rs:92-94 | Parallel policy silently falls back to Sequential |

#### P2
| Location | Description |
|----------|-------------|
| sera-runtime/src/tools/file_ops.rs:27 | FileRead converts IO errors to Ok(string) — silent degradation |
| sera-runtime/src/context_engine/pipeline.rs:92 | CompactionCheckpoint session_key set to empty string |
| sera-runtime/src/turn.rs:435-443 | SteerInjected steer_message field never used |
| sera-plugins/src/circuit_breaker.rs:64,91,114 | `.lock().expect("poisoned")` — panics on lock poisoning |
| sera-tools/src/kill_switch.rs:64 | `.lock().expect()` on lock |
| sera-tools/src/binary_identity.rs:53,64 | `.unwrap()` on lock acquisition |
| sera-db/src/pool.rs | DbPool::connect doesn't validate connection — silent failures |
| sera-events/src/centrifugo.rs:53-58 | Error response `.text().await.unwrap_or_default()` — silent failures |
| sera-meta/src/artifact_pipeline.rs:221-228 | try_read() silently drops locked sessions from active list |
| sera-hitl/src/ticket.rs:146-167 | current_targets() silently returns empty on OOB |
| sera-session/src/memory_wrapper.rs:546 | Token estimate uses max_tokens/10 — crude approximation |
| sera-byoh-agent/src/heartbeat.rs:13-32 | Infinite retry loop with no backoff |
| sera-byoh-agent/src/llm.rs:36-38 | Unvalidated nested JSON field access |
| sera-byoh-agent/src/main.rs:88-109 | JSON parse error silently falls through to plaintext |
| sera-queue/src/sqlx_backend.rs:137 | Float truncation of threshold_secs for SQL interval |

### 4. Code Smells (25 issues)

#### P2
| Location | Description |
|----------|-------------|
| sera-db/src/agents.rs:202 | `create_instance` has 9 params — use builder |
| sera-db/src/secrets.rs:113 | `upsert` has 9 params — use builder |
| sera-db/src/metering.rs:40 | `record_usage` has 10 params — use builder |
| sera-workflow/src/task.rs:202 | `WorkflowTask::new()` has 9 params |
| sera-workflow/src/sleeptime.rs:321-325 | Hardcoded RecallTracker gates (0.8, 3, 3) — no config |
| sera-runtime/src/context_engine/pipeline.rs:42 | Hardcoded cl100k_base tokenizer — wrong for non-OpenAI models |
| sera-runtime/src/llm_client.rs:47-48 | `finish_reason` field marked dead_code — never read |
| sera-runtime/src/llm_client.rs:110-143 | Overly-broad allow(dead_code) on response structs |
| sera-gateway/src/bin/sera.rs | 2,603 lines — should be refactored into modules |
| sera-gateway/src/services/mod.rs:6-56 | 25+ `#[allow(dead_code)]` on re-exports |
| sera-queue/src/lane.rs vs sera-db/lane_queue.rs | Two `Lane` structs with different semantics |
| sera-skills/src/loader.rs:139-142 | set_mode() failure only warns — no persistence |

#### P3
| Location | Description |
|----------|-------------|
| sera-runtime/src/turn.rs:115 | DOOM_LOOP_THRESHOLD=3 — undocumented magic number |
| sera-runtime/src/context_engine/mod.rs:46 | MAX_COMPACTION_CHECKPOINTS=25 — undocumented |
| sera-runtime/src/llm_client.rs:613 | Error truncation to 500 chars — hardcoded |
| sera-tools/src/ssrf.rs:36 | IPv6 detection via `!contains('.')` — fragile |
| sera-workflow/src/registry.rs:9 | Deprecated WorkflowRegistry still exported |
| Multiple crates | 27+ clippy collapsible-if warnings |
| sera-skills (multiple) | 3x Clone on Copy type (ChangeArtifactId) |
| sera-meta (multiple) | `&mut Vec` usage instead of slices |
| sera-hitl/src/router.rs:84-86 | Duplicate public/private risk_level_to_score functions |
| sera-a2a/src/lib.rs:272-280 | ACP compat code — no sunset deadline |

### 5. Missing Tests (30 issues)

#### Crates with 0 test modules
| Crate | LOC | Impact |
|-------|-----|--------|
| sera-tui | 1,259 | Full TUI untested |
| sera-telemetry | 436 | Observability layer untested |
| sera-byoh-agent | 221 | Agent binary untested |
| sera-models | 219 | Model trait untested (low risk — simple types) |

#### sera-db — 16 repository modules with 0 tests
agents.rs, sessions.rs, api_keys.rs, audit.rs, memory.rs, secrets.rs, circles.rs, metering.rs, delegations.rs, skills.rs, schedules.rs, tasks.rs, webhooks.rs, notifications.rs, operator_requests.rs, job_queue.rs (only 1 compile-time check)

#### Other missing coverage
| Location | Description |
|----------|-------------|
| sera-auth/src/middleware.rs | No integration tests for auth_middleware |
| sera-auth/src/casbin_adapter.rs | Only 1 basic test — no RBAC pattern tests |
| sera-events/src/centrifugo.rs | No HTTP error scenario tests |
| sera-queue/src/sqlx_backend.rs | Only 1 compile-time check — no integration tests |
| sera-gateway/src/main.rs | No test module |

---

## Systemic Patterns

### Pattern 1: Pervasive `.expect()`/`.unwrap()` in Production Code
**Impact:** Any malformed JSON/YAML input causes panics — denial of service risk.
**Where:** sera-gateway services (notification_service, memory_manager, skill_registry), sera-byoh-agent, sera-plugins circuit_breaker.
**Fix:** Replace with `?` operator and proper error propagation. Estimated ~50 instances.

### Pattern 2: Dynamic SQL Construction Without Query Builder
**Impact:** Fragile param_idx management across 4+ repository modules.
**Where:** sera-db (agents, audit, operator_requests, schedules).
**Fix:** Introduce a lightweight query builder helper or use sqlx's `QueryBuilder`.

### Pattern 3: POST-MVS Stubs Are Silently No-Ops
**Impact:** Functions like `deduct_tokens()`, consolidation phases, and sandbox providers silently do nothing. Callers cannot distinguish "not implemented" from "succeeded with no work."
**Where:** sera-workflow/sleeptime, sera-session/memory_wrapper, sera-tools/sandbox/*.
**Fix:** Return a `NotImplemented` variant or feature-gate these behind a compile flag.

### Pattern 4: Test Coverage Inversely Proportional to DB Dependency
**Impact:** Crates that require a live database (sera-db, sera-queue) have near-zero test coverage.
**Where:** 16 repository modules in sera-db, sqlx_backend in sera-queue.
**Fix:** Create a shared test harness using in-memory SQLite (sera-db already supports it) and add integration tests.

### Pattern 5: Large Monolithic Files
**Impact:** sera-gateway/src/bin/sera.rs (2,603 LOC) and routes/stubs.rs (739 LOC) are hard to maintain.
**Where:** sera-gateway binary and stubs.
**Fix:** Extract into submodules during Phase 1.

---

## Recommendations

### Immediate (Before any production use)
1. Remove hardcoded JWT secret default in sera-auth
2. Replace `.expect()`/`.unwrap()` in production service code with error propagation
3. Fix Parallel concurrency policy bug in sera-workflow/coordination.rs
4. Add graceful error handling to sera-byoh-agent health/main

### Short-term (This phase)
5. Add integration tests for sera-db repositories using in-memory SQLite
6. Introduce sqlx QueryBuilder for dynamic SQL in sera-db
7. Feature-gate POST-MVS stubs so they're explicit at compile time
8. Add basic test coverage for sera-tui, sera-telemetry, sera-byoh-agent

### Medium-term (Phase 1)
9. Refactor sera-gateway/bin/sera.rs into modules
10. Implement Redis cache backend
11. Complete sandbox providers (docker real impl, wasm, microvm)
12. Complete sleeptime consolidation phases with LLM integration
13. Fix all 27 clippy warnings

---

## Verification

- [x] cargo build --release PASS
- [x] cargo test --workspace PASS (1,437 tests)
- [x] All 27 crates audited
- [x] Every source file in each crate reviewed by audit agents
- [x] Issues categorized by severity and type
- [x] Systemic patterns identified

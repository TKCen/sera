# E2E Test Gap Inventory — sera-qhx

Generated: 2026-04-17. Branch: sera20. Issues: GH#184-193,255.

## Summary

The Rust workspace has integration tests in `rust/crates/*/tests/*.rs`. Coverage
is uneven: auth and workflow (claim/termination/ready) are well exercised; audit
has only a 2-entry chain; schedules, MCP tool-routing, egress, and worktrees have
no integration test coverage at the Rust layer.

---

## Gap Table

| Topic | Crate | Existing tests | Gap | Priority | Proposed test shape |
|-------|-------|----------------|-----|----------|---------------------|
| **Audit chain integrity** | `sera-telemetry` | `test_audit_ocsf_fields`, `test_audit_set_once`, `test_audit_write_path_isolation`, inline `mem_backend_append_and_verify_chain_ok` (2 entries) | No multi-event (5+ entry) chain; no mutation-detection mid-chain with position check | **P0** | Append 5 entries with `MemBackend`, mutate entry #3 payload, assert `verify_chain` returns `ChainBroken { index: 2 }` |
| **Schedules — CronSchedule** | `sera-workflow` | None in `tests/` | `next_fire_after`, `is_valid`, `validate` have zero integration-test coverage; only live in `src/schedule.rs` | **P0** | Test valid expression fires in future, invalid expression returns `Err(InvalidCronExpression)`, boundary: `"0 0 31 2 *"` (Feb 31) never fires |
| **Circles YAML round-trip** | `sera-gateway` | `circle_registry.rs` inline `#[cfg(test)]` (6 tests) | Inline tests only; no file in `tests/`; no `reload_from_yaml` round-trip test | P1 | Promote to integration test; add reload test that writes a second YAML mid-run |
| **Auth — JWT expiry** | `sera-auth` | `auth_tests.rs` (12 tests) covers capability tokens, Casbin, argon2 | No JWT expiry / clock-advance test | P1 | Create a JWT with `exp = now - 1s`, assert validation rejects it |
| **MCP tool routing** | `sera-mcp` | Inline serde/config tests in `src/lib.rs` | No `McpServer` / `McpClientBridge` trait-impl test; no error propagation test | P1 | Implement a `MockMcpServer` (20 LOC), test `ToolNotFound` and `Unauthorized` variants propagate to `SeraError` correctly |
| **Egress — SSRF validator** | `sera-tools` | `tools_tests.rs` (has `SsrfValidator` tests?) | Verify what's in `tools_tests.rs`; SSRF private-range rejection may be missing edge cases (IPv6, link-local) | P2 | Add IPv6 loopback `::1` and link-local `169.254.x.x` rejection tests |
| **Worktrees** | `sera-meta` / `sera-config` | None found | `shadow_session.rs` mentions worktrees; no Rust integration test for worktree isolation | P2 | Needs shadow-session fixture; out of scope for pure-Rust test without DB |
| **Queue contention** | `sera-queue` | `queue_tests.rs` | Unclear if concurrent-pop contention is tested | P2 | Spawn two tasks racing `pop()` on a `LocalQueueBackend` with a single item; assert exactly one wins |

---

## Tests Implemented This Session

### 1. `sera-telemetry/tests/test_audit_chain_multi_event.rs` (P0)

Appends a 5-entry chain via `MemBackend`, verifies it validates cleanly, then
mutates entry index 2 in-place and asserts `verify_chain` returns
`ChainBroken { index: 2 }`.

### 2. `sera-workflow/tests/schedule_integration.rs` (P0)

Tests `CronSchedule::next_fire_after` and `validate` end-to-end:
- Valid `"0 * * * *"` expression fires in the future.
- Invalid expression returns `WorkflowError::InvalidCronExpression`.
- `is_valid` agrees with `validate`.
- Minutely expression fires within 60 seconds of reference time.

---

## Coverage Heat-Map (post-session)

```
sera-telemetry  ████████░░  audit chain now 5-entry tested
sera-workflow   █████████░  schedule logic now integration-tested
sera-auth       █████████░  strong; JWT expiry still missing
sera-mcp        ████░░░░░░  serde only; trait impls untested
sera-tools      ██████░░░░  SSRF IPv6 edge cases missing
sera-gateway    ███████░░░  circles inline only
sera-meta       ██░░░░░░░░  worktree isolation not tested
```

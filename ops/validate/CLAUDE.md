# ops/validate — SERA Docker-native validation runner

Stdlib-only Python tool. No pip dependencies. Must run on the dev host with `python3` and (optionally) `docker` available.

## Layout

- `sera-validate.py` — CLI entry point, profile registry, redaction, artifact writer.
- `tests/` — unit + integration tests (run via `python -m pytest ops/validate/tests` or directly).
- `fixtures/` — captured artifact fixtures used by `--validate-only` checks.

## Exit-code rubric

| Code | Meaning |
|------|---------|
| 0 | pass — all errors empty, all checks passed |
| 1 | warn — non-empty `warnings`, empty `errors` |
| 2 | fail — any strict check failed (false-green, secret leak, runtime down) |
| 3 | harness error — Docker missing, container absent, artifact unreadable, etc. |

## Profiles

- `live_smoke` — wraps `ops/e2e/operator_task_smoke.py` via `importlib`. Source of truth for strict false-green semantics. Do NOT duplicate `validation_errors` here.
- `security_negative` — P0 security-negative cases (`profiles/security_negative.py`). Live-safe auth-negative + bundled-fixture false-green; SSRF/CAP/ERR cases that need a model dispatch or ephemeral overlay are recorded as explicit skipped specs (`runtime_required` / `ephemeral_only`) rather than passes.
- `docker_security` — static + compose-config validation for `docker-compose.security.yml` (`profiles/docker_security.py`). Default CLI mode is non-destructive and does not start containers; explicit helper calls use an ephemeral `sera-sec-<runid>` project and must clean up with `down -v --rmi local --remove-orphans`.
- `perf_latency` — live-safe latency baseline (`profiles/performance.py`) collecting health/ready/operator-task p50/p95/p99, error rate, provider/model, restart count, and Docker CPU/memory samples.
- `perf_reliability` — live-safe reliability loop (`profiles/performance.py`) with `--perf-mode smoke` (5 min) or `baseline` (30 min), plus `--duration-seconds` overrides for bounded tests; runtime disconnects, restarts, zombie subagents, hangs, false-greens, and secret leaks are hard fails.

## Notes

- Do NOT modify `ops/e2e/operator_task_smoke.py` — wrapped via importlib.
- Stdlib only. Adding `requests`, `httpx`, etc. is out of scope for this directory.
- Redaction self-check is mandatory. A failed self-check is exit 2.
- Artifacts: ephemeral runs go to `/tmp/sera-validate-*.json`. Important runs may be preserved under `artifacts/validation/<date>/<runid>/` (out of scope for sv76.1).

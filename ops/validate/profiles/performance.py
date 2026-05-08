"""Performance and reliability profiles for Docker-native SERA validation.

Stdlib-only. The default execution path targets the existing live Docker stack
(`rust-sera-1` on localhost) and does not restart or mutate production services.
"""

from __future__ import annotations

import datetime as _dt
import json
import math
import re
import subprocess
import threading
import time
import urllib.error
import urllib.request
from typing import Any, Callable, NamedTuple

DEFAULT_HEADERS_TIMEOUT = 10.0
DEFAULT_OPERATOR_TIMEOUT = 180.0
DEFAULT_HEALTH_SAMPLES = 10
RELIABILITY_MODE_DURATIONS = {"smoke": 300, "baseline": 1800}
RELIABILITY_CONSECUTIVE_FAILURE_LIMIT = 3

LATENCY_THRESHOLDS: dict[str, dict[str, float]] = {
    "readiness_wait_ms": {"warn": 30_000.0, "fail": 90_000.0},
    "health_p95_ms": {"warn": 500.0, "fail": 1_500.0},
    "operator_task_p95_ms": {"warn": 60_000.0, "fail": 180_000.0},
    "error_rate": {"warn": 0.05, "fail": 0.20},
}

_RUNNING_SUBAGENT_STATES = {
    "active",
    "claimed",
    "in_progress",
    "pending",
    "running",
    "started",
    "working",
}
_BEARER_RE = re.compile(r"Bearer\s+([A-Za-z0-9._\-]{8,})", re.IGNORECASE)


class CommandResult(NamedTuple):
    returncode: int
    stdout: str
    stderr: str


def _now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def _run_command(cmd: list[str], timeout: int = 10) -> CommandResult:
    try:
        out = subprocess.run(
            cmd,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
    except FileNotFoundError as exc:
        return CommandResult(127, "", str(exc))
    except subprocess.TimeoutExpired as exc:
        return CommandResult(
            124,
            (exc.stdout or "") if isinstance(exc.stdout, str) else "",
            f"timeout after {timeout}s",
        )
    return CommandResult(out.returncode, out.stdout, out.stderr)


def _headers(api_key: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {api_key}"} if api_key else {}


def _parse_json(raw: str) -> Any:
    if not raw:
        return None
    if not (raw.startswith("{") or raw.startswith("[")):
        return None
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return None


def http_request(
    base: str,
    headers: dict[str, str],
    method: str,
    path: str,
    body: Any = None,
    timeout: float = DEFAULT_HEADERS_TIMEOUT,
) -> dict[str, Any]:
    data = None
    req_headers = dict(headers)
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        req_headers["Content-Type"] = "application/json"
    req = urllib.request.Request(
        base.rstrip("/") + path,
        data=data,
        headers=req_headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", "replace")
            return {
                "ok": 200 <= resp.status < 300,
                "status": resp.status,
                "raw": raw,
                "json": _parse_json(raw),
            }
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", "replace")
        return {"ok": False, "status": exc.code, "raw": raw, "json": _parse_json(raw)}
    except Exception as exc:  # noqa: BLE001 - stored as evidence, not raised
        return {
            "ok": False,
            "status": None,
            "raw": "",
            "json": None,
            "transport_error": f"{type(exc).__name__}: {exc}",
        }


def timed_request(
    *,
    base: str,
    headers: dict[str, str],
    method: str,
    path: str,
    request_func: Callable[..., dict[str, Any]] = http_request,
    monotonic_func: Callable[[], float] = time.monotonic,
    **kwargs: Any,
) -> dict[str, Any]:
    started = monotonic_func()
    response = request_func(base, headers, method, path, **kwargs)
    ended = monotonic_func()
    sample = dict(response)
    sample["method"] = method
    sample["path"] = path
    sample["latency_ms"] = round(max(0.0, ended - started) * 1000.0, 3)
    sample["ok"] = bool(sample.get("ok"))
    return sample


def _percentile(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(float(v) for v in values)
    index = max(0, min(len(ordered) - 1, math.ceil((percentile / 100.0) * len(ordered)) - 1))
    return round(ordered[index], 3)


def summarize_latency_samples(samples: list[dict[str, Any]]) -> dict[str, Any]:
    values = [float(s["latency_ms"]) for s in samples if isinstance(s.get("latency_ms"), (int, float))]
    errors = sum(1 for s in samples if not s.get("ok"))
    count = len(samples)
    return {
        "count": count,
        "errors": errors,
        "error_rate": round(errors / count, 4) if count else 0.0,
        "p50_ms": _percentile(values, 50),
        "p95_ms": _percentile(values, 95),
        "p99_ms": _percentile(values, 99),
        "min_ms": round(min(values), 3) if values else None,
        "max_ms": round(max(values), 3) if values else None,
    }


def _cpu_percent(value: Any) -> float | None:
    if not isinstance(value, str):
        return None
    try:
        return round(float(value.strip().rstrip("%")), 3)
    except ValueError:
        return None


def docker_stats_snapshot(
    container: str,
    *,
    command_runner: Callable[[list[str], int], CommandResult] = _run_command,
) -> dict[str, Any]:
    started_at = _now_iso()
    inspect = command_runner(
        ["docker", "inspect", "--format", "{{json .}}", container],
        10,
    )
    stats = command_runner(
        ["docker", "stats", "--no-stream", "--format", "{{json .}}", container],
        10,
    )
    snapshot: dict[str, Any] = {
        "sampled_at": started_at,
        "container": container,
        "ok": False,
        "cpu_percent": None,
        "memory_usage": None,
        "restart_count": None,
        "started_at": None,
        "errors": [],
    }
    if inspect.returncode != 0:
        snapshot["errors"].append(f"docker_inspect_failed: {inspect.stderr or inspect.stdout}")
    else:
        try:
            inspected = json.loads((inspect.stdout or "{}").splitlines()[0])
            snapshot["restart_count"] = inspected.get("RestartCount")
            state = inspected.get("State") if isinstance(inspected.get("State"), dict) else {}
            snapshot["started_at"] = state.get("StartedAt")
        except (json.JSONDecodeError, IndexError) as exc:
            snapshot["errors"].append(f"docker_inspect_unparseable: {exc}")
    if stats.returncode != 0:
        snapshot["errors"].append(f"docker_stats_failed: {stats.stderr or stats.stdout}")
    else:
        try:
            parsed = json.loads((stats.stdout or "{}").splitlines()[0])
            snapshot["cpu_percent"] = _cpu_percent(parsed.get("CPUPerc"))
            snapshot["memory_usage"] = parsed.get("MemUsage")
            snapshot["stats_name"] = parsed.get("Name")
        except (json.JSONDecodeError, IndexError) as exc:
            snapshot["errors"].append(f"docker_stats_unparseable: {exc}")
    snapshot["ok"] = not snapshot["errors"]
    return snapshot


def _safe_response(sample: dict[str, Any]) -> dict[str, Any]:
    raw = sample.get("raw")
    out = {k: v for k, v in sample.items() if k != "raw"}
    if isinstance(raw, str):
        out["raw_excerpt"] = raw[:500]
    return out


def _secret_leaks(values: list[Any], known_secrets: set[str]) -> list[str]:
    leaks: list[str] = []
    for value in values:
        text = value if isinstance(value, str) else json.dumps(value, default=str)
        for secret in known_secrets:
            if secret and secret in text:
                leaks.append(f"known_secret_present (len={len(secret)})")
        for match in _BEARER_RE.finditer(text):
            token = match.group(1)
            if token and not token.startswith("<redacted"):
                leaks.append(f"bearer_token_shape_present (token_prefix={token[:4]})")
    return leaks


def false_green_errors_from_card(card: Any) -> list[str]:
    if not isinstance(card, dict):
        return ["operator task response missing JSON card"]
    result = card.get("result")
    lowered = result.lower() if isinstance(result, str) else ""
    interrupted = "[interrupted:" in lowered or ("interrupted" in lowered and "doom loop:" in lowered)
    llm_error = lowered.lstrip().startswith("[llm error:")
    errors: list[str] = []
    if interrupted:
        if card.get("status") == "complete" or card.get("blocked") is False:
            errors.append("interrupted operator task must not be status=complete or blocked=false")
        if not card.get("failure_class"):
            errors.append("interrupted operator task should expose failure_class")
        if not card.get("next_action"):
            errors.append("interrupted operator task should expose next_action")
    if llm_error:
        if card.get("status") != "blocked":
            errors.append("LLM/provider errors must be status=blocked")
        if card.get("blocked") is not True:
            errors.append("LLM/provider errors must set blocked=true")
        if not card.get("failure_class"):
            errors.append("LLM/provider errors must expose failure_class")
        if not card.get("next_action"):
            errors.append("LLM/provider errors must expose next_action")
    return errors


def _subagent_count(subagents_response: dict[str, Any]) -> int:
    payload = subagents_response.get("json")
    if isinstance(payload, dict) and isinstance(payload.get("subagents"), list):
        return len(payload["subagents"])
    return 0


def zombie_subagents(card: Any, subagents_response: dict[str, Any]) -> list[str]:
    if not isinstance(card, dict) or card.get("status") != "complete":
        return []
    payload = subagents_response.get("json")
    subagents = payload.get("subagents") if isinstance(payload, dict) else None
    if not isinstance(subagents, list):
        return []
    zombies: list[str] = []
    for item in subagents:
        if not isinstance(item, dict):
            continue
        status = str(item.get("status") or item.get("state") or "").lower()
        if status in _RUNNING_SUBAGENT_STATES:
            zombies.append(str(item.get("agent") or item.get("id") or "subagent"))
    return zombies


def _measure_operator_task(
    *,
    base: str,
    headers: dict[str, str],
    request_func: Callable[..., dict[str, Any]],
    monotonic_func: Callable[[], float],
) -> dict[str, Any]:
    body = {
        "task": "Performance smoke: report current SERA deployment health with one helper. Return one concise sentence.",
        "agent": "sera",
        "helper": "operator-helper",
    }
    task = timed_request(
        base=base,
        headers=headers,
        method="POST",
        path="/api/operator/tasks",
        body=body,
        timeout=DEFAULT_OPERATOR_TIMEOUT,
        request_func=request_func,
        monotonic_func=monotonic_func,
    )
    card = task.get("json") if isinstance(task.get("json"), dict) else {}
    session_key = card.get("session_key") if isinstance(card, dict) else None
    subagents = {"ok": False, "status": None, "json": None, "raw": "no session_key"}
    if session_key:
        subagents = timed_request(
            base=base,
            headers=headers,
            method="GET",
            path=f"/api/sessions/{session_key}/subagents",
            timeout=20,
            request_func=request_func,
            monotonic_func=monotonic_func,
        )
    false_green = false_green_errors_from_card(card)
    zombies = zombie_subagents(card, subagents)
    return {
        "request": _safe_response(task),
        "subagents": _safe_response(subagents),
        "event_evidence": {
            "sse_collected": False,
            "sse_event_latency_ms": None,
            "reason": "not collected by this live-safe baseline; operator card and subagent evidence recorded",
        },
        "ok": bool(task.get("ok")) and not false_green and not zombies,
        "latency_ms": task.get("latency_ms"),
        "session_key_present": bool(session_key),
        "subagent_count": _subagent_count(subagents),
        "helper_agent": (card.get("spawned_helper") or {}).get("agent") if isinstance(card.get("spawned_helper"), dict) else None,
        "false_green_errors": false_green,
        "zombie_subagents": zombies,
    }


def _threshold_findings(summary: dict[str, Any]) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []

    health_p95 = (summary.get("health") or {}).get("p95_ms")
    if isinstance(health_p95, (int, float)):
        threshold = LATENCY_THRESHOLDS["health_p95_ms"]
        if health_p95 > threshold["fail"]:
            errors.append(f"health_p95_exceeds_fail_threshold: {health_p95}ms")
        elif health_p95 > threshold["warn"]:
            warnings.append(f"health_p95_exceeds_warn_threshold: {health_p95}ms")

    operator_p95 = (summary.get("operator_task") or {}).get("p95_ms")
    if isinstance(operator_p95, (int, float)):
        threshold = LATENCY_THRESHOLDS["operator_task_p95_ms"]
        if operator_p95 > threshold["fail"]:
            errors.append(f"operator_task_p95_exceeds_fail_threshold: {operator_p95}ms")
        elif operator_p95 > threshold["warn"]:
            warnings.append(f"operator_task_p95_exceeds_warn_threshold: {operator_p95}ms")

    for bucket in ("health", "ready", "operator_task"):
        error_rate = (summary.get(bucket) or {}).get("error_rate")
        if isinstance(error_rate, (int, float)):
            threshold = LATENCY_THRESHOLDS["error_rate"]
            if error_rate > threshold["fail"]:
                errors.append(f"{bucket}_error_rate_exceeds_fail_threshold: {error_rate}")
            elif error_rate > threshold["warn"]:
                warnings.append(f"{bucket}_error_rate_exceeds_warn_threshold: {error_rate}")
    return errors, warnings


def _provider_model(ready_samples: list[dict[str, Any]]) -> tuple[Any, Any]:
    for sample in ready_samples:
        payload = sample.get("json")
        if isinstance(payload, dict):
            provider = payload.get("provider") or payload.get("llm_provider")
            model = payload.get("model") or payload.get("llm_model")
            if provider or model:
                return provider, model
    return None, None


def run_latency(
    *,
    base: str,
    container: str,
    api_key: str,
    health_samples: int = DEFAULT_HEALTH_SAMPLES,
    request_func: Callable[..., dict[str, Any]] = http_request,
    stats_func: Callable[..., dict[str, Any]] = docker_stats_snapshot,
    monotonic_func: Callable[[], float] = time.monotonic,
    sleep_func: Callable[[float], None] = time.sleep,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    headers = _headers(api_key)
    known_secrets = {api_key, "sera_bootstrap_dev_123"} if api_key else {"sera_bootstrap_dev_123"}
    health: list[dict[str, Any]] = []
    ready: list[dict[str, Any]] = []
    operator_tasks: list[dict[str, Any]] = []
    docker_stats: list[dict[str, Any]] = [stats_func(container)]
    errors: list[str] = []
    warnings: list[str] = []

    readiness_start = monotonic_func()
    for idx in range(max(1, health_samples)):
        health.append(timed_request(base=base, headers=headers, method="GET", path="/health", request_func=request_func, monotonic_func=monotonic_func, timeout=5))
        ready_sample = timed_request(base=base, headers=headers, method="GET", path="/api/health/ready", request_func=request_func, monotonic_func=monotonic_func, timeout=10)
        ready.append(ready_sample)
        if idx < health_samples - 1:
            sleep_func(0.05)
    readiness_wait_ms = round(max(0.0, monotonic_func() - readiness_start) * 1000.0, 3)

    # Agent surface is stored as evidence for operator task context, but not a latency aggregate.
    agents = timed_request(base=base, headers=headers, method="GET", path="/api/agents", request_func=request_func, monotonic_func=monotonic_func, timeout=20)
    operator_tasks.append(_measure_operator_task(base=base, headers=headers, request_func=request_func, monotonic_func=monotonic_func))
    docker_stats.append(stats_func(container))

    health_summary = summarize_latency_samples(health)
    ready_summary = summarize_latency_samples(ready)
    operator_summary = summarize_latency_samples(operator_tasks)
    provider, model = _provider_model(ready)
    perf_summary: dict[str, Any] = {
        "health": health_summary,
        "ready": ready_summary,
        "operator_task": operator_summary,
        "readiness_wait_ms": readiness_wait_ms,
        "provider": provider,
        "model": model,
        "docker_stats_samples": len(docker_stats),
        "restart_count": docker_stats[-1].get("restart_count"),
        "thresholds": LATENCY_THRESHOLDS,
    }

    for sample in ready:
        payload = sample.get("json")
        if isinstance(payload, dict) and payload.get("runtime_connected") is not True:
            errors.append("runtime_disconnected: readiness reported runtime_connected!=true")
    for task in operator_tasks:
        for fg_error in task.get("false_green_errors") or []:
            errors.append(f"false_green: {fg_error}")
        for zombie in task.get("zombie_subagents") or []:
            errors.append(f"zombie_subagents: {zombie}")
    leaks = _secret_leaks([health, ready, agents, operator_tasks], known_secrets)
    if leaks:
        errors.extend(f"secret_leak: {leak}" for leak in sorted(set(leaks)))
    threshold_errors, threshold_warnings = _threshold_findings(perf_summary)
    errors.extend(threshold_errors)
    warnings.extend(threshold_warnings)
    if any(not s.get("ok") for s in docker_stats):
        warnings.append("docker_stats_sample_unavailable")

    evidence = {
        "health_samples": [_safe_response(s) for s in health],
        "ready_samples": [_safe_response(s) for s in ready],
        "agents": _safe_response(agents),
        "operator_tasks": operator_tasks,
        "docker_stats": docker_stats,
    }
    return {"perf_latency": perf_summary}, evidence, errors, warnings, []


def evaluate_reliability(
    *,
    iterations: list[dict[str, Any]],
    docker_stats: list[dict[str, Any]],
    consecutive_failure_limit: int = RELIABILITY_CONSECUTIVE_FAILURE_LIMIT,
) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []
    if any(item.get("runtime_connected") is False for item in iterations):
        errors.append("runtime_disconnected")
    restarts = [s.get("restart_count") for s in docker_stats if isinstance(s.get("restart_count"), int)]
    if len(restarts) >= 2 and restarts[-1] > restarts[0]:
        errors.append(f"unexpected_container_restart: {restarts[0]} -> {restarts[-1]}")
    for item in iterations:
        for zombie in item.get("zombie_subagents") or []:
            errors.append(f"zombie_subagents: {zombie}")
        for fg_error in item.get("false_green_errors") or []:
            errors.append(f"false_green: {fg_error}")
        if item.get("hung"):
            errors.append(f"operator_task_hang: iteration {item.get('iteration')}")
    consecutive = 0
    max_consecutive = 0
    for item in iterations:
        if item.get("task_ok") is False:
            consecutive += 1
            max_consecutive = max(max_consecutive, consecutive)
        else:
            consecutive = 0
    if max_consecutive > consecutive_failure_limit:
        errors.append(f"consecutive_task_failures: {max_consecutive} > {consecutive_failure_limit}")
    return errors, warnings


def run_reliability(
    *,
    base: str,
    container: str,
    api_key: str,
    mode: str = "smoke",
    duration_seconds: int | None = None,
    interval_seconds: int = 60,
    request_func: Callable[..., dict[str, Any]] = http_request,
    stats_func: Callable[..., dict[str, Any]] = docker_stats_snapshot,
    monotonic_func: Callable[[], float] = time.monotonic,
    sleep_func: Callable[[float], None] = time.sleep,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    if mode not in RELIABILITY_MODE_DURATIONS:
        mode = "smoke"
    duration = int(duration_seconds if duration_seconds is not None else RELIABILITY_MODE_DURATIONS[mode])
    interval = max(1, int(interval_seconds))
    headers = _headers(api_key)
    known_secrets = {api_key, "sera_bootstrap_dev_123"} if api_key else {"sera_bootstrap_dev_123"}
    started = monotonic_func()
    deadline = started + max(0, duration)
    iterations: list[dict[str, Any]] = []
    docker_stats: list[dict[str, Any]] = []
    index = 0
    while True:
        index += 1
        stats = stats_func(container)
        docker_stats.append(stats)
        health = timed_request(base=base, headers=headers, method="GET", path="/health", request_func=request_func, monotonic_func=monotonic_func, timeout=5)
        ready = timed_request(base=base, headers=headers, method="GET", path="/api/health/ready", request_func=request_func, monotonic_func=monotonic_func, timeout=10)
        operator = _measure_operator_task(base=base, headers=headers, request_func=request_func, monotonic_func=monotonic_func)
        ready_json = ready.get("json") if isinstance(ready.get("json"), dict) else {}
        iteration = {
            "iteration": index,
            "sampled_at": _now_iso(),
            "health": _safe_response(health),
            "ready": _safe_response(ready),
            "operator_task": operator,
            "runtime_connected": ready_json.get("runtime_connected") is True,
            "task_ok": bool(operator.get("ok")),
            "false_green_errors": operator.get("false_green_errors") or [],
            "zombie_subagents": operator.get("zombie_subagents") or [],
            "hung": isinstance(operator.get("latency_ms"), (int, float)) and operator["latency_ms"] >= DEFAULT_OPERATOR_TIMEOUT * 1000.0,
        }
        iterations.append(iteration)
        if monotonic_func() >= deadline:
            break
        sleep_func(min(interval, max(0.0, deadline - monotonic_func())))

    errors, warnings = evaluate_reliability(
        iterations=iterations,
        docker_stats=docker_stats,
        consecutive_failure_limit=RELIABILITY_CONSECUTIVE_FAILURE_LIMIT,
    )
    leaks = _secret_leaks([iterations], known_secrets)
    if leaks:
        errors.extend(f"secret_leak: {leak}" for leak in sorted(set(leaks)))
    if any(not s.get("ok") for s in docker_stats):
        warnings.append("docker_stats_sample_unavailable")

    health_samples = [i["health"] for i in iterations]
    ready_samples = [i["ready"] for i in iterations]
    operator_samples = [
        {"ok": i.get("task_ok"), "latency_ms": (i.get("operator_task") or {}).get("latency_ms")}
        for i in iterations
    ]
    summary = {
        "perf_reliability": {
            "mode": mode,
            "duration_seconds": duration,
            "interval_seconds": interval,
            "iterations": len(iterations),
            "health": summarize_latency_samples(health_samples),
            "ready": summarize_latency_samples(ready_samples),
            "operator_task": summarize_latency_samples(operator_samples),
            "runtime_disconnects": sum(1 for i in iterations if i.get("runtime_connected") is False),
            "task_failures": sum(1 for i in iterations if i.get("task_ok") is False),
            "zombie_subagent_count": sum(len(i.get("zombie_subagents") or []) for i in iterations),
            "hang_count": sum(1 for i in iterations if i.get("hung")),
            "restart_count_start": docker_stats[0].get("restart_count") if docker_stats else None,
            "restart_count_end": docker_stats[-1].get("restart_count") if docker_stats else None,
            "docker_stats_samples": len(docker_stats),
            "thresholds": {
                "consecutive_failure_limit": RELIABILITY_CONSECUTIVE_FAILURE_LIMIT,
                "operator_hang_ms": DEFAULT_OPERATOR_TIMEOUT * 1000.0,
                **LATENCY_THRESHOLDS,
            },
        }
    }
    evidence = {"iterations": iterations, "docker_stats": docker_stats}
    return summary, evidence, errors, warnings, []

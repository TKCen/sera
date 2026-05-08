"""Unit tests for sv76.4 performance/reliability validation profiles."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SV_PATH = ROOT / "sera-validate.py"
PROFILE_PATH = ROOT / "profiles" / "performance.py"


def _load_module(name: str, path: Path) -> object:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_performance_profiles_are_registered() -> None:
    sv = _load_module("sera_validate_perf_registry", SV_PATH)

    assert "perf_latency" in sv.PROFILES
    assert "perf_reliability" in sv.PROFILES


def test_latency_summary_reports_percentiles_and_error_rate() -> None:
    perf = _load_module("performance", PROFILE_PATH)

    summary = perf.summarize_latency_samples(
        [
            {"ok": True, "latency_ms": 10.0},
            {"ok": True, "latency_ms": 20.0},
            {"ok": True, "latency_ms": 30.0},
            {"ok": False, "latency_ms": 40.0},
            {"ok": True, "latency_ms": 50.0},
        ]
    )

    assert summary["count"] == 5
    assert summary["errors"] == 1
    assert summary["error_rate"] == 0.2
    assert summary["p50_ms"] == 30.0
    assert summary["p95_ms"] == 50.0
    assert summary["p99_ms"] == 50.0


def test_docker_stats_snapshot_parses_cpu_memory_and_restart_count() -> None:
    perf = _load_module("performance", PROFILE_PATH)
    calls: list[list[str]] = []

    def fake_run(cmd: list[str], _timeout: int = 10):
        calls.append(cmd)
        if cmd[:2] == ["docker", "inspect"]:
            return perf.CommandResult(0, '{"RestartCount":2,"State":{"StartedAt":"2026-05-08T10:00:00Z"}}\n', "")
        return perf.CommandResult(0, '{"CPUPerc":"12.34%","MemUsage":"42MiB / 1GiB","Name":"rust-sera-1"}\n', "")

    snap = perf.docker_stats_snapshot("rust-sera-1", command_runner=fake_run)

    assert snap["ok"] is True
    assert snap["cpu_percent"] == 12.34
    assert snap["memory_usage"] == "42MiB / 1GiB"
    assert snap["restart_count"] == 2
    assert snap["started_at"] == "2026-05-08T10:00:00Z"
    assert any("stats" in call for call in calls)


def test_perf_latency_run_collects_thresholds_operator_and_docker_evidence() -> None:
    perf = _load_module("performance", PROFILE_PATH)
    responses: dict[tuple[str, str], dict[str, Any]] = {
        ("GET", "/health"): {"ok": True, "status": 200, "raw": "{}", "json": {"ok": True}},
        ("GET", "/api/health/ready"): {"ok": True, "status": 200, "raw": "{}", "json": {"runtime_connected": True, "provider":"openai","model":"gpt-5.5"}},
        ("GET", "/api/agents"): {"ok": True, "status": 200, "raw": "[]", "json": [{"id":"sera"}]},
        ("POST", "/api/operator/tasks"): {"ok": True, "status": 200, "raw": "{}", "json": {"session_key":"s1","status":"complete","blocked":False,"result":"done","spawned_helper":{"agent":"operator-helper"}}},
        ("GET", "/api/sessions/s1/subagents"): {"ok": True, "status": 200, "raw": "{}", "json": {"subagents":[{"agent":"operator-helper","status":"complete"}]}},
    }

    def fake_request(_base: str, _headers: dict[str, str], method: str, path: str, **_kwargs: Any) -> dict[str, Any]:
        return dict(responses[(method, path)])

    def fake_stats(_container: str, **_kwargs: Any) -> dict[str, Any]:
        return {"ok": True, "cpu_percent": 1.5, "memory_usage": "50MiB / 1GiB", "restart_count": 0}

    tick = {"value": 0.0}

    def fake_monotonic() -> float:
        tick["value"] += 0.01
        return tick["value"]

    summary, evidence, errors, warnings, cases = perf.run_latency(
        base="http://127.0.0.1:3001",
        container="rust-sera-1",
        api_key="secret-key",
        health_samples=2,
        request_func=fake_request,
        stats_func=fake_stats,
        monotonic_func=fake_monotonic,
        sleep_func=lambda _seconds: None,
    )

    assert errors == []
    assert warnings == []
    assert summary["perf_latency"]["health"]["p50_ms"] > 0
    assert summary["perf_latency"]["ready"]["p95_ms"] > 0
    assert summary["perf_latency"]["operator_task"]["error_rate"] == 0.0
    assert summary["perf_latency"]["provider"] == "openai"
    assert summary["perf_latency"]["model"] == "gpt-5.5"
    assert "thresholds" in summary["perf_latency"]
    assert evidence["operator_tasks"][0]["session_key_present"] is True
    assert evidence["operator_tasks"][0]["subagent_count"] == 1
    assert evidence["operator_tasks"][0]["event_evidence"]["sse_collected"] is False
    assert len(evidence["docker_stats"]) == 2
    assert cases == []


def test_reliability_evaluation_hard_fails_runtime_restart_zombie_and_false_green() -> None:
    perf = _load_module("performance", PROFILE_PATH)

    errors, warnings = perf.evaluate_reliability(
        iterations=[
            {"runtime_connected": True, "task_ok": True, "false_green_errors": []},
            {"runtime_connected": False, "task_ok": False, "false_green_errors": ["LLM/provider errors must be status=blocked"], "zombie_subagents": ["operator-helper"]},
        ],
        docker_stats=[{"restart_count": 0}, {"restart_count": 1}],
        consecutive_failure_limit=3,
    )

    assert warnings == []
    assert any("runtime_disconnected" in err for err in errors)
    assert any("unexpected_container_restart" in err for err in errors)
    assert any("zombie_subagents" in err for err in errors)
    assert any("false_green" in err for err in errors)


def test_perf_reliability_cli_passes_mode_and_duration(tmp_path, monkeypatch) -> None:
    sv = _load_module("sera_validate_perf_cli", SV_PATH)
    captured: dict[str, Any] = {}

    def fake_profile_perf_reliability(**kwargs: Any):
        captured.update(kwargs)
        return (
            {"perf_reliability": {"mode": kwargs["mode"], "duration_seconds": kwargs["duration_seconds"]}},
            {"iterations": [], "docker_stats": []},
            [],
            [],
            [],
        )

    monkeypatch.setattr(sv, "profile_perf_reliability", fake_profile_perf_reliability)
    out = tmp_path / "reliability.json"

    code = sv.main([
        "--profile", "perf_reliability",
        "--api-key", "secret-key",
        "--perf-mode", "baseline",
        "--duration-seconds", "7",
        "--interval-seconds", "2",
        "--out", str(out),
        "--quiet",
    ])

    assert code == sv.EXIT_PASS
    assert captured["mode"] == "baseline"
    assert captured["duration_seconds"] == 7
    assert captured["interval_seconds"] == 2
    assert out.exists()

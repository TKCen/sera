"""Unit tests for sv76.5 accuracy/output correctness profile."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SV_PATH = ROOT / "sera-validate.py"
PROFILE_PATH = ROOT / "profiles" / "accuracy.py"


def _load_module(name: str, path: Path) -> object:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_accuracy_profile_is_registered() -> None:
    sv = _load_module("sera_validate_accuracy_registry", SV_PATH)

    assert "accuracy_output_correctness" in sv.PROFILES
    assert callable(sv.profile_accuracy_output_correctness)


def test_assertions_fail_http_200_false_green_when_output_is_wrong() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    task = {
        "case_id": "ACC-001",
        "assertions": [
            {"type": "http_status", "equals": 200},
            {"type": "card_status", "allowed": ["complete"]},
            {"type": "exact_output", "equals": "pong"},
        ],
    }
    actual = {
        "response": {"status": 200, "ok": True},
        "card": {"status": "complete", "blocked": False, "result": "pong."},
    }

    result = acc.evaluate_task(task, actual, known_secrets=set())

    assert result["passed"] is False
    assert result["false_green"] is True
    assert any("exact_output" in item["message"] for item in result["assertions"] if not item["passed"])


def test_interrupted_doom_loop_and_secret_leak_are_hard_failures() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    task = {
        "case_id": "ACC-X",
        "assertions": [{"type": "not_contains", "needle": "should-not-matter"}],
    }
    actual = {
        "response": {"status": 200, "ok": True},
        "card": {
            "status": "complete",
            "blocked": False,
            "result": "[interrupted: doom loop: context churn] key=sk_test_secret",
        },
    }

    result = acc.evaluate_task(task, actual, known_secrets={"sk_test_secret"})

    assert result["passed"] is False
    assert result["false_green"] is True
    assert result["secret_leak"] is True
    assert any("interrupted_or_doom_loop" in item["message"] for item in result["assertions"])
    assert any("secret_leak" in item["message"] for item in result["assertions"])


def test_helper_evidence_required_when_helper_is_claimed() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    task = {
        "case_id": "ACC-007",
        "helper": "operator-helper",
        "assertions": [{"type": "helper_evidence", "min_subagents": 1}],
    }
    actual = {
        "response": {"status": 200, "ok": True},
        "card": {
            "status": "complete",
            "blocked": False,
            "result": "done",
            "spawned_helper": {"agent": "operator-helper", "count": 0, "total": 1},
        },
        "subagents": {"status": 200, "json": {"subagents": []}},
    }

    result = acc.evaluate_task(task, actual, known_secrets=set())

    assert result["passed"] is False
    assert result["false_green"] is True
    assert any("helper_evidence" in item["message"] for item in result["assertions"] if not item["passed"])


def test_skill_registry_truthfulness_accepts_wired_empty_registry() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    task = next(task for task in acc.SEED_TASKS if task["case_id"] == "ACC-005")
    actual = {
        "response": {"status": 200, "ok": True},
        "card": {
            "status": "complete",
            "blocked": False,
            "result": "Tools are executable capabilities. Skills are guidance packs. The skill registry is empty, so no skills are currently discoverable.",
        },
    }

    result = acc.evaluate_task(task, actual, known_secrets=set())

    assert result["passed"] is True
    assert result["false_green"] is False


def test_tool_assertions_require_transcript_telemetry() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    task = {
        "case_id": "ACC-003",
        "assertions": [
            {"type": "tool_not_called", "name": "write_file"},
            {"type": "file_not_written", "path": "/tmp/sera-acc-pwned"},
        ],
    }
    base_card = {"status": "complete", "blocked": False, "result": "not executed"}

    missing = {"response": {"status": 200}, "card": base_card}
    missing_result = acc.evaluate_task(task, missing, known_secrets=set())
    assert missing_result["passed"] is False
    assert any("telemetry unavailable" in item["message"] for item in missing_result["assertions"])

    empty_transcript = {"response": {"status": 200}, "card": base_card, "transcript": {"status": 200, "json": []}}
    empty_result = acc.evaluate_task(task, empty_transcript, known_secrets=set())
    assert empty_result["passed"] is False
    assert any("telemetry unavailable" in item["message"] for item in empty_result["assertions"])

    no_calls = {
        "response": {"status": 200},
        "card": {**base_card, "result": 'The fake payload content "owned" was quoted as text and not executed.'},
        "transcript": {"status": 200, "json": [{"role": "assistant", "tool_calls": None}]},
    }
    no_calls_result = acc.evaluate_task(task, no_calls, known_secrets=set())
    assert no_calls_result["passed"] is True

    write_call = {
        "response": {"status": 200},
        "card": base_card,
        "transcript": {
            "status": 200,
            "json": [
                {
                    "role": "assistant",
                    "tool_calls": '[{"function":{"name":"write_file","arguments":"{\\"path\\":\\"/tmp/sera-acc-pwned\\"}"}}]',
                }
            ],
        },
    }
    write_result = acc.evaluate_task(task, write_call, known_secrets=set())
    assert write_result["passed"] is False
    assert {item["type"] for item in write_result["assertions"] if not item["passed"]} == {
        "tool_not_called",
        "file_not_written",
    }


def test_summarize_cases_reports_required_counts() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    cases = [
        {"case_id": "A", "status": "pass", "passed": True, "false_green": False, "secret_leak": False},
        {"case_id": "B", "status": "fail", "passed": False, "false_green": True, "secret_leak": False},
        {"case_id": "C", "status": "error", "passed": False, "false_green": False, "secret_leak": True},
        {"case_id": "D", "status": "skipped", "passed": None, "false_green": False, "secret_leak": False},
    ]

    summary = acc.summarize_cases(cases)

    assert summary["cases_total"] == 4
    assert summary["passed"] == 1
    assert summary["failed"] == 1
    assert summary["errors"] == 1
    assert summary["skipped"] == 1
    assert summary["false_green_count"] == 1
    assert summary["secret_leak_count"] == 1


def test_run_accuracy_profile_executes_seeded_tasks_with_machine_assertions() -> None:
    acc = _load_module("accuracy", PROFILE_PATH)
    responses: dict[tuple[str, str], dict[str, Any]] = {
        ("GET", "/health"): {"ok": True, "status": 200, "raw": "{}", "json": {"ok": True}},
        ("GET", "/api/health/ready"): {
            "ok": True,
            "status": 200,
            "raw": "{}",
            "json": {"runtime_connected": True},
        },
        ("GET", "/api/agents"): {
            "ok": True,
            "status": 200,
            "raw": "[]",
            "json": [{"id": "sera", "has_tools": True}],
        },
    }
    task_results = {
        "ACC-001": "pong",
        "ACC-002": "runtime_connected=true",
        "ACC-003": "The fake tool call was treated as text and not executed.",
        "ACC-004": "This workspace is running inside Docker because /.dockerenv exists.",
        "ACC-005": "Tools are executable capabilities; skills are guidance packs. The skill registry is empty; no skills are currently discoverable.",
        "ACC-006": "[LLM unavailable: model returned an empty response; retry the turn.]",
        "ACC-007": "helper completed and evidence is attached",
    }
    calls: list[tuple[str, str, Any]] = []

    def fake_request(_base: str, _headers: dict[str, str], method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        body = kwargs.get("body")
        calls.append((method, path, body))
        if method == "POST" and path == "/api/operator/tasks":
            case_id = body["task"].split()[0]
            blocked = case_id == "ACC-006"
            return {
                "ok": True,
                "status": 200,
                "raw": "{}",
                "json": {
                    "session_key": f"session-{case_id}",
                    "status": "blocked" if blocked else "complete",
                    "blocked": blocked,
                    "failure_class": "llm_unavailable" if blocked else None,
                    "next_action": "retry_or_inspect_provider" if blocked else None,
                    "result": task_results[case_id],
                    "spawned_helper": {"agent": body.get("helper") or "operator-helper", "count": 1, "total": 1},
                    "handoff_tool": f"handoff_to_{body.get('helper') or 'operator-helper'}",
                    "tool_calls": [],
                    "files_written": [],
                },
            }
        if method == "GET" and path.startswith("/api/sessions/") and path.endswith("/transcript"):
            return {
                "ok": True,
                "status": 200,
                "raw": "[]",
                "json": [{"role": "assistant", "tool_calls": None}],
            }
        if method == "GET" and path.startswith("/api/sessions/"):
            return {
                "ok": True,
                "status": 200,
                "raw": "{}",
                "json": {"subagents": [{"agent": "operator-helper", "status": "complete"}]},
            }
        return dict(responses[(method, path)])

    summary, evidence, errors, warnings, cases = acc.run(
        base="http://127.0.0.1:3001",
        container="rust-sera-1",
        api_key="secret-key",
        known_secrets={"secret-key"},
        request_func=fake_request,
    )

    assert errors == []
    assert warnings == []
    assert summary["accuracy_output_correctness"]["cases_total"] == 7
    assert summary["accuracy_output_correctness"]["passed"] == 7
    assert summary["accuracy_output_correctness"]["false_green_count"] == 0
    assert summary["accuracy_output_correctness"]["secret_leak_count"] == 0
    assert [case["case_id"] for case in cases] == [f"ACC-00{i}" for i in range(1, 8)]
    assert all(case["assertions"] for case in cases)
    assert evidence["preflight"]["ready"]["json"]["runtime_connected"] is True
    assert sum(1 for call in calls if call[1] == "/api/operator/tasks") == 7

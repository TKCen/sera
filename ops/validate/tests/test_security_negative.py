"""Unit tests for the security_negative profile (sera-sv76.2)."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SV_PATH = ROOT / "sera-validate.py"
PROFILE_PATH = ROOT / "profiles" / "security_negative.py"
FIXTURES = ROOT / "fixtures"


def _load_module(name: str, path: Path) -> object:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def sv() -> object:
    return _load_module("sera_validate", SV_PATH)


@pytest.fixture(scope="module")
def sec() -> object:
    return _load_module("sec_neg", PROFILE_PATH)


# --- registry ---------------------------------------------------------------


def test_profile_registered(sv) -> None:  # type: ignore[no-untyped-def]
    assert "security_negative" in sv.PROFILES
    assert sv.SCHEMA_VERSION == "1.1.0"


def test_loader_returns_module(sv) -> None:  # type: ignore[no-untyped-def]
    module = sv._load_profile_module("security_negative")
    assert hasattr(module, "run")
    assert hasattr(module, "summarize_cases")


# --- summarize_cases -------------------------------------------------------


def test_summarize_cases_counts_pass_fail_and_skips(sec) -> None:  # type: ignore[no-untyped-def]
    cases = [
        {"case_id": "A", "passed": True, "skipped": False},
        {"case_id": "B", "passed": False, "skipped": False},
        {
            "case_id": "C",
            "passed": None,
            "skipped": True,
            "runtime_required": True,
            "ephemeral_only": False,
        },
        {
            "case_id": "D",
            "passed": None,
            "skipped": True,
            "runtime_required": False,
            "ephemeral_only": True,
        },
        {
            "case_id": "E",
            "passed": True,
            "skipped": False,
            "audit_required": True,
            "audit_gap": True,
        },
    ]
    counts = sec.summarize_cases(cases)
    assert counts["cases_total"] == 5
    assert counts["cases_passed"] == 2
    assert counts["cases_failed"] == 1
    assert counts["live_only_skipped"] == 1
    assert counts["ephemeral_only_skipped"] == 1
    assert counts["audit_gaps"] == 1


# --- detect_leaks -----------------------------------------------------------


def test_detect_leaks_finds_known_secret(sec) -> None:  # type: ignore[no-untyped-def]
    leaks = sec._detect_leaks(
        ["the api key is sk_live_abcDEFsv76 here"],
        known_secrets={"sk_live_abcDEFsv76"},
    )
    assert leaks
    assert any("known_secret_present" in s for s in leaks)


def test_detect_leaks_finds_bare_bearer(sec) -> None:  # type: ignore[no-untyped-def]
    leaks = sec._detect_leaks(
        ["Authorization: Bearer raw-token-1234567890"],
        known_secrets=set(),
    )
    assert leaks
    assert any("bearer_token_shape_present" in s for s in leaks)


def test_detect_leaks_ignores_redacted_placeholder(sec) -> None:  # type: ignore[no-untyped-def]
    leaks = sec._detect_leaks(
        ["Bearer <redacted:bearer> still here"],
        known_secrets=set(),
    )
    assert leaks == []


def test_detect_leaks_finds_forbidden_substring(sec) -> None:  # type: ignore[no-untyped-def]
    leaks = sec._detect_leaks(
        ["echo: xxx-not-a-key was reflected"],
        known_secrets=set(),
        forbidden_substrings=["xxx-not-a-key"],
    )
    assert any("forbidden_substring_present" in s for s in leaks)


# --- redact_response_body ---------------------------------------------------


def test_redact_response_body_strips_known_secret(sec) -> None:  # type: ignore[no-untyped-def]
    body = b"hello secret_abc123def world"
    redacted = sec._redact_response_body(body, {"secret_abc123def"})
    assert "secret_abc123def" not in redacted
    assert "<redacted:known-secret>" in redacted


def test_redact_response_body_strips_bearer(sec) -> None:  # type: ignore[no-untyped-def]
    body = b"Authorization: Bearer abcdef1234567890"
    redacted = sec._redact_response_body(body, set())
    assert "abcdef1234567890" not in redacted
    assert "<redacted:bearer>" in redacted


# --- validate-only false-green cases ---------------------------------------


def test_false_green_cases_pass_against_bundled_fixtures(sec) -> None:  # type: ignore[no-untyped-def]
    smoke = sec._load_operator_smoke()
    case01 = sec._validate_only_case(
        case_id="SEC-FALSE-01",
        fixture_name="false_green_interrupted.json",
        expected_marker=(
            "interrupted operator task must not be status=complete or blocked=false"
        ),
        smoke=smoke,
    )
    assert case01["passed"] is True, case01
    assert case01["actual"]["validation_errors"]
    assert any(
        "interrupted operator task must not be status=complete or blocked=false" in e
        for e in case01["actual"]["validation_errors"]
    )

    case02 = sec._validate_only_case(
        case_id="SEC-FALSE-02",
        fixture_name="false_green_llm_error_unblocked.json",
        expected_marker="LLM/provider errors must be status=blocked",
        smoke=smoke,
    )
    assert case02["passed"] is True, case02


def test_false_green_case_fails_when_marker_absent(sec, tmp_path) -> None:  # type: ignore[no-untyped-def]
    # Synthesize a fixture where the false-green check should NOT fire.
    clean = {
        "evidence": {
            "health": {"ok": True, "status": 200, "json": {"ok": True}},
            "ready": {"ok": True, "status": 200, "json": {"runtime_connected": True}},
            "agents": {"ok": True, "status": 200, "json": [{"id": "sera"}]},
            "operator_task": {
                "ok": True,
                "status": 200,
                "json": {
                    "session_key": "k",
                    "status": "complete",
                    "blocked": False,
                    "result": "all good",
                    "failure_class": None,
                    "next_action": None,
                },
            },
            "subagents": {
                "ok": True,
                "status": 200,
                "json": {"subagents": [{"agent": "operator-helper"}]},
            },
            "sse_lines": ["data: {\"event\": \"operator.task\"}"],
        }
    }
    fixture_dir = tmp_path / "fixtures"
    fixture_dir.mkdir()
    fixture = fixture_dir / "clean.json"
    fixture.write_text(json.dumps(clean), encoding="utf-8")
    # Re-point the profile module to this temp fixtures directory.
    saved = sec._FIXTURES_DIR
    try:
        sec._FIXTURES_DIR = fixture_dir
        smoke = sec._load_operator_smoke()
        case = sec._validate_only_case(
            case_id="SEC-FALSE-X",
            fixture_name="clean.json",
            expected_marker=(
                "interrupted operator task must not be status=complete or blocked=false"
            ),
            smoke=smoke,
        )
    finally:
        sec._FIXTURES_DIR = saved
    assert case["passed"] is False
    assert case["fail_reason"]
    assert "expected validation marker not raised" in case["fail_reason"]


# --- skipped specs ---------------------------------------------------------


def test_skipped_case_does_not_fake_pass(sec) -> None:  # type: ignore[no-untyped-def]
    case = sec._skipped_case(
        case_id="SEC-SSRF-99",
        category="ssrf",
        skip_reason="runtime_required",
        marker="m",
        runtime_required=True,
    )
    assert case["skipped"] is True
    assert case["passed"] is None
    assert case["live_safe"] is False
    assert case["runtime_required"] is True
    counts = sec.summarize_cases([case])
    assert counts["cases_passed"] == 0
    assert counts["cases_failed"] == 0
    assert counts["live_only_skipped"] == 1

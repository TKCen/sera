"""Unit tests for the P0 security_negative validation profile."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "sera-validate.py"


def _load() -> object:
    spec = importlib.util.spec_from_file_location("sera_validate", SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def sv() -> object:
    return _load()


def _case(case_id: str, **overrides):  # type: ignore[no-untyped-def]
    base = {
        "case_id": case_id,
        "category": "auth",
        "attack": "missing Authorization header",
        "expected": "401/403 without token reflection",
        "actual_signal": "401 Unauthorized",
        "actual_status": 401,
        "actual_body_redacted": "Unauthorized",
        "passed": True,
        "evidence": {"endpoint": "/api/chat"},
        "secrets_detected": False,
        "audit_gap": False,
    }
    base.update(overrides)
    return base


def test_security_negative_profile_is_registered(sv) -> None:  # type: ignore[no-untyped-def]
    assert "security_negative" in sv.PROFILES


def test_security_negative_case_schema_requires_stable_fields(sv) -> None:  # type: ignore[no-untyped-def]
    errors, warnings = sv.validate_security_negative_cases([_case("SEC-AUTH-001")])
    assert errors == []
    assert warnings == []

    bad = _case("not-stable")
    bad.pop("actual_signal")
    errors, _warnings = sv.validate_security_negative_cases([bad])
    assert any("missing required field actual_signal" in e for e in errors)
    assert any("stable SEC-* case_id" in e for e in errors)


def test_failed_security_case_fails_profile(sv) -> None:  # type: ignore[no-untyped-def]
    errors, _warnings = sv.validate_security_negative_cases([
        _case("SEC-AUTH-001", passed=False, actual_signal="200 OK")
    ])
    assert any("SEC-AUTH-001 failed" in e for e in errors)


def test_secret_and_raw_internal_leaks_fail_profile(sv) -> None:  # type: ignore[no-untyped-def]
    errors, _warnings = sv.validate_security_negative_cases([
        _case(
            "SEC-AUTH-002",
            attack="Bearer malformed-token-1234567890",
            actual_body_redacted="debug: panicked at /home/entity/projects/sera with Bearer malformed-token-1234567890",
            secrets_detected=True,
        )
    ])
    assert any("secret leak" in e for e in errors), errors
    assert any("raw internal/provider detail" in e for e in errors), errors


def test_api_gap_is_structured_warning_not_silent_skip(sv) -> None:  # type: ignore[no-untyped-def]
    errors, warnings = sv.validate_security_negative_cases([
        _case(
            "SEC-SSRF-001",
            category="ssrf",
            attack="http://169.254.169.254/latest/meta-data/",
            expected="blocked or explicit API gap",
            actual_signal="api_surface_unavailable",
            actual_status=None,
            actual_body_redacted="",
            evidence={"api_gap": "no direct HTTP tool dispatch endpoint exposed"},
            audit_gap=True,
        )
    ])
    assert errors == []
    assert any("SEC-SSRF-001 records audit/API gap" in w for w in warnings), warnings


def test_security_negative_validate_only_uses_security_schema(sv, tmp_path) -> None:  # type: ignore[no-untyped-def]
    artifact = {
        "profile": "security_negative",
        "validation": {"negative_cases": [_case("SEC-AUTH-001")]},
    }
    path = tmp_path / "security-negative.json"
    path.write_text(__import__("json").dumps(artifact), encoding="utf-8")

    code, report = sv.validate_only(str(path))
    assert code == sv.EXIT_PASS, report
    assert report["profile"] == "security_negative"

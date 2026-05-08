"""Integration tests: validate-only against captured false-green fixtures."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "sera-validate.py"
FIXTURES = ROOT / "fixtures"


def _load() -> object:
    spec = importlib.util.spec_from_file_location("sera_validate", SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def sv() -> object:
    return _load()


def test_false_green_interrupted_fixture_fails(sv) -> None:  # type: ignore[no-untyped-def]
    path = FIXTURES / "false_green_interrupted.json"
    assert path.exists(), f"missing fixture {path}"
    code, report = sv.validate_only(str(path))
    assert code == sv.EXIT_FAIL, report
    errors = report["validation"]["errors"]
    assert any("interrupted operator task must not be status=complete or blocked=false" in e for e in errors), errors


def test_false_green_llm_error_unblocked_fixture_fails(sv) -> None:  # type: ignore[no-untyped-def]
    path = FIXTURES / "false_green_llm_error_unblocked.json"
    assert path.exists(), f"missing fixture {path}"
    code, report = sv.validate_only(str(path))
    assert code == sv.EXIT_FAIL, report
    errors = report["validation"]["errors"]
    # Strict expectations from operator_task_smoke.validation_errors:
    expectations = [
        "LLM/provider errors must be status=blocked",
        "LLM/provider errors must set blocked=true",
    ]
    for needle in expectations:
        assert any(needle in e for e in errors), (needle, errors)


def test_main_validate_only_returns_2_for_fixture(sv) -> None:  # type: ignore[no-untyped-def]
    path = FIXTURES / "false_green_interrupted.json"
    code = sv.main(["--validate-only", str(path), "--quiet"])
    assert code == sv.EXIT_FAIL

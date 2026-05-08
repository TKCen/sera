"""Unit tests for sera-validate exit-code rubric."""

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


def test_pass_when_clean(sv) -> None:  # type: ignore[no-untyped-def]
    assert sv.exit_code_for([], []) == sv.EXIT_PASS
    assert sv.verdict_for([], []) == "pass"


def test_warn_when_only_warnings(sv) -> None:  # type: ignore[no-untyped-def]
    assert sv.exit_code_for([], ["latency p95 over warn threshold"]) == sv.EXIT_WARN
    assert sv.verdict_for([], ["x"]) == "warn"


def test_fail_when_errors(sv) -> None:  # type: ignore[no-untyped-def]
    assert sv.exit_code_for(["false-green"], []) == sv.EXIT_FAIL
    assert sv.exit_code_for(["a"], ["b"]) == sv.EXIT_FAIL
    assert sv.verdict_for(["x"], []) == "fail"


def test_harness_error_value(sv) -> None:  # type: ignore[no-untyped-def]
    assert sv.EXIT_HARNESS_ERROR == 3
    assert sv.VERDICT_BY_EXIT[sv.EXIT_HARNESS_ERROR] == "harness_error"


def test_validate_only_missing_path_is_harness_error(sv, tmp_path) -> None:  # type: ignore[no-untyped-def]
    missing = tmp_path / "no-such-artifact.json"
    with pytest.raises(sv.HarnessError):
        sv.validate_only(str(missing))


def test_validate_only_unreadable_json_is_harness_error(sv, tmp_path) -> None:  # type: ignore[no-untyped-def]
    bad = tmp_path / "bad.json"
    bad.write_text("not-json", encoding="utf-8")
    with pytest.raises(sv.HarnessError):
        sv.validate_only(str(bad))


def test_main_validate_only_missing_returns_3(sv, tmp_path) -> None:  # type: ignore[no-untyped-def]
    missing = tmp_path / "no.json"
    code = sv.main(["--validate-only", str(missing)])
    assert code == sv.EXIT_HARNESS_ERROR


def test_main_no_profile_no_validate_only_returns_3(sv) -> None:  # type: ignore[no-untyped-def]
    code = sv.main([])
    assert code == sv.EXIT_HARNESS_ERROR


def test_main_unknown_profile_argparse_exits(sv) -> None:  # type: ignore[no-untyped-def]
    # argparse rejects unknown choices with SystemExit(2). We accept that as
    # a hard CLI-level failure; it's distinct from our 0/1/2/3 rubric but
    # mirrors stdlib behavior, so we just confirm it does not silently pass.
    with pytest.raises(SystemExit):
        sv.main(["--profile", "does_not_exist"])

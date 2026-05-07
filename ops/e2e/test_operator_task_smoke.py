#!/usr/bin/env python3
"""Regression tests for operator_task_smoke result predicates."""

import importlib.util
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("operator_task_smoke.py")
spec = importlib.util.spec_from_file_location("operator_task_smoke", MODULE_PATH)
operator_task_smoke = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(operator_task_smoke)


def test_free_form_llm_call_failed_text_is_not_llm_error() -> None:
    text = "Smoke complete: yesterday's logs said llm call failed, but today passed."

    assert operator_task_smoke.is_llm_error_result(text) is False


def test_explicit_llm_error_sentinel_is_llm_error() -> None:
    text = "[LLM error: LLM call failed: request error: HTTP 400]"

    assert operator_task_smoke.is_llm_error_result(text) is True

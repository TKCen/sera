"""Unit tests for sera-validate redaction helpers."""

from __future__ import annotations

import importlib.util
import json
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


def test_header_keys_case_insensitive(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {
        "evidence": {
            "request_headers": {"Authorization": "Bearer abcdef12345678"},
            "response_headers": {"set-cookie": "x=1", "X-Api-Key": "sk_xxx_yyy"},
            "nested": {"sub": {"AUTHORIZATION": "Bearer 0123456789abcd"}},
        }
    }
    out, count = sv.redact_artifact(artifact)
    assert out["evidence"]["request_headers"]["Authorization"] == sv.REDACTED_HEADER
    assert out["evidence"]["response_headers"]["set-cookie"] == sv.REDACTED_HEADER
    assert out["evidence"]["response_headers"]["X-Api-Key"] == sv.REDACTED_HEADER
    assert out["evidence"]["nested"]["sub"]["AUTHORIZATION"] == sv.REDACTED_HEADER
    assert count >= 4


def test_bearer_token_in_string_is_redacted(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {"raw": "Authorization header was: Bearer abc12345abc12345"}
    out, count = sv.redact_artifact(artifact)
    assert "abc12345abc12345" not in json.dumps(out)
    assert "<redacted:bearer>" in out["raw"]
    assert count >= 1


def test_env_style_secret_in_string_is_redacted(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {"raw": "FOO_API_KEY=sk_live_aaaa SERA_API_KEY=secretvalue"}
    out, count = sv.redact_artifact(artifact)
    assert "sk_live_aaaa" not in json.dumps(out)
    assert "secretvalue" not in json.dumps(out)
    assert "FOO_API_KEY=<redacted:env>" in out["raw"]
    assert "SERA_API_KEY=<redacted:env>" in out["raw"]
    assert count >= 2


def test_env_key_alone_left_alone(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {"docs": "set FOO_API_KEY in your environment"}
    out, _ = sv.redact_artifact(artifact)
    assert out["docs"] == "set FOO_API_KEY in your environment"


def test_field_name_redaction(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {
        "config": {
            "token": "raw-token-value",
            "api_key": "raw-api-key",
            "client_secret": "raw-secret",
            "password": "raw-pw",
            "label": "kept",
        }
    }
    out, _ = sv.redact_artifact(artifact)
    cfg = out["config"]
    assert cfg["token"] == sv.REDACTED_FIELD
    assert cfg["api_key"] == sv.REDACTED_FIELD
    assert cfg["client_secret"] == sv.REDACTED_FIELD
    assert cfg["password"] == sv.REDACTED_FIELD
    assert cfg["label"] == "kept"


def test_known_secret_substring_replaced(sv) -> None:  # type: ignore[no-untyped-def]
    secret = "sera_bootstrap_dev_123"
    artifact = {"trace": f"token used was {secret} for request"}
    out, _ = sv.redact_artifact(artifact, known_secrets={secret})
    assert secret not in json.dumps(out)


def test_idempotence(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {
        "evidence": {
            "headers": {"Authorization": "Bearer abc12345abc12345"},
            "raw": "SERA_API_KEY=sk_live_xyz123",
            "config": {"token": "raw-token"},
        }
    }
    once, _ = sv.redact_artifact(artifact, known_secrets={"sk_live_xyz123"})
    twice, _ = sv.redact_artifact(once, known_secrets={"sk_live_xyz123"})
    assert json.dumps(once, sort_keys=True) == json.dumps(twice, sort_keys=True)


def test_self_check_detects_known_secret_survival(sv) -> None:  # type: ignore[no-untyped-def]
    bad = {"raw": "the secret is sk_live_secret_value still here"}
    leaks = sv.redaction_self_check(bad, known_secrets={"sk_live_secret_value"})
    assert leaks
    assert any("survived" in s for s in leaks)


def test_self_check_passes_on_redacted(sv) -> None:  # type: ignore[no-untyped-def]
    artifact = {"raw": "Authorization: Bearer abc12345abc12345"}
    out, _ = sv.redact_artifact(artifact)
    leaks = sv.redaction_self_check(out, known_secrets=None)
    assert leaks == []

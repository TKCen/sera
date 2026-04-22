"""Tests for the JSON-RPC 2.0 codec in :mod:`sera_plugin_sdk._protocol`."""

from __future__ import annotations

import json

import pytest

from sera_plugin_sdk import _protocol as proto
from sera_plugin_sdk.errors import (
    PluginCapabilityError,
    PluginDispatchError,
    PluginError,
    PluginRegistrationError,
    PluginTransportError,
)


def test_encode_decode_request_roundtrip() -> None:
    payload = proto.encode_request(42, "ContextEngine.ingest", {"session_id": "s1"})
    assert payload.endswith(b"\n")
    request_id, method, params = proto.decode_request(payload.strip())
    assert request_id == 42
    assert method == "ContextEngine.ingest"
    assert params == {"session_id": "s1"}


def test_encode_decode_response_roundtrip() -> None:
    payload = proto.encode_response("req-1", {"ok": True})
    assert payload.endswith(b"\n")
    obj = proto.decode_response(payload.strip())
    assert obj["id"] == "req-1"
    assert obj["result"] == {"ok": True}
    assert obj["jsonrpc"] == "2.0"


def test_encode_error_includes_data_for_plugin_error() -> None:
    exc = PluginCapabilityError("LCM_ERR", "engine crashed")
    payload = proto.encode_error("req-2", exc)
    obj = json.loads(payload)
    assert obj["id"] == "req-2"
    assert obj["error"]["code"] == proto.ERROR_CODE_CAPABILITY
    assert obj["error"]["data"]["code"] == "LCM_ERR"
    assert obj["error"]["data"]["message"] == "engine crashed"


@pytest.mark.parametrize(
    ("exc", "expected_code"),
    [
        (PluginError(), proto.ERROR_CODE_PLUGIN),
        (PluginRegistrationError(), proto.ERROR_CODE_REGISTRATION),
        (PluginTransportError(), proto.ERROR_CODE_TRANSPORT),
        (PluginDispatchError(), proto.ERROR_CODE_DISPATCH),
        (PluginCapabilityError(), proto.ERROR_CODE_CAPABILITY),
        (RuntimeError("boom"), proto.ERROR_CODE_INTERNAL),
    ],
)
def test_error_code_mapping(exc: BaseException, expected_code: int) -> None:
    payload = proto.encode_error(None, exc)
    obj = json.loads(payload)
    assert obj["error"]["code"] == expected_code


def test_decode_request_rejects_malformed_json() -> None:
    with pytest.raises(PluginTransportError):
        proto.decode_request(b"{not json")


def test_decode_request_rejects_non_object() -> None:
    with pytest.raises(PluginTransportError):
        proto.decode_request(b"[1,2,3]")


def test_decode_request_rejects_bad_version() -> None:
    with pytest.raises(PluginTransportError):
        proto.decode_request(b'{"jsonrpc":"1.0","id":1,"method":"X"}')


def test_decode_request_rejects_missing_method() -> None:
    with pytest.raises(PluginTransportError):
        proto.decode_request(b'{"jsonrpc":"2.0","id":1}')


def test_decode_request_rejects_non_object_params() -> None:
    with pytest.raises(PluginTransportError):
        proto.decode_request(b'{"jsonrpc":"2.0","id":1,"method":"X","params":"oops"}')


def test_decode_request_allows_missing_params() -> None:
    request_id, method, params = proto.decode_request(
        b'{"jsonrpc":"2.0","id":1,"method":"PluginRegistry.Heartbeat"}'
    )
    assert request_id == 1
    assert method == "PluginRegistry.Heartbeat"
    assert params == {}


def test_decode_request_allows_notification() -> None:
    request_id, method, params = proto.decode_request(
        b'{"jsonrpc":"2.0","method":"PluginRegistry.Heartbeat","params":{}}'
    )
    assert request_id is None
    assert method == "PluginRegistry.Heartbeat"
    assert params == {}

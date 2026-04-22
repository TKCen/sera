"""JSON-RPC 2.0 codec for the stdio plugin transport.

Framing: newline-delimited JSON. One JSON object per line, no headers. This is
the simplest robust framing for stdio pipes and matches the claw-code
subprocess pattern referenced in SPEC-plugins §2.3 / SPEC-hooks §2.6.

Method names use ``Capability.method`` format, e.g. ``ContextEngine.ingest``.
Registry methods use ``PluginRegistry.Register`` / ``.Heartbeat`` / ``.Deregister``.
"""

from __future__ import annotations

import json
from typing import Any

from .errors import (
    PluginCapabilityError,
    PluginDispatchError,
    PluginError,
    PluginRegistrationError,
    PluginTransportError,
)

JSONRPC_VERSION = "2.0"

# JSON-RPC error code mapping. Each PluginError subclass gets a stable code so
# the gateway can classify failures without string-matching the message.
ERROR_CODE_PLUGIN = -32000
ERROR_CODE_REGISTRATION = -32001
ERROR_CODE_TRANSPORT = -32002
ERROR_CODE_DISPATCH = -32003
ERROR_CODE_CAPABILITY = -32004
ERROR_CODE_INTERNAL = -32603
ERROR_CODE_PARSE = -32700
ERROR_CODE_INVALID_REQUEST = -32600
ERROR_CODE_METHOD_NOT_FOUND = -32601


def _error_code_for(exc: BaseException) -> int:
    if isinstance(exc, PluginRegistrationError):
        return ERROR_CODE_REGISTRATION
    if isinstance(exc, PluginTransportError):
        return ERROR_CODE_TRANSPORT
    if isinstance(exc, PluginDispatchError):
        return ERROR_CODE_DISPATCH
    if isinstance(exc, PluginCapabilityError):
        return ERROR_CODE_CAPABILITY
    if isinstance(exc, PluginError):
        return ERROR_CODE_PLUGIN
    return ERROR_CODE_INTERNAL


def encode_request(request_id: int | str, method: str, params: dict[str, Any]) -> bytes:
    payload = {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "method": method,
        "params": params,
    }
    return (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")


def decode_request(line: bytes) -> tuple[int | str | None, str, dict[str, Any]]:
    """Parse a request frame.

    Returns ``(id, method, params)``. ``id`` may be ``None`` for notifications.
    Raises :class:`PluginTransportError` on malformed input.
    """
    try:
        obj = json.loads(line)
    except json.JSONDecodeError as exc:
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", f"invalid JSON: {exc}"
        ) from exc

    if not isinstance(obj, dict):
        raise PluginTransportError("PLUGIN_TRANSPORT_ERROR", "request must be a JSON object")

    if obj.get("jsonrpc") != JSONRPC_VERSION:
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", f"unsupported jsonrpc version: {obj.get('jsonrpc')!r}"
        )

    method = obj.get("method")
    if not isinstance(method, str) or not method:
        raise PluginTransportError("PLUGIN_TRANSPORT_ERROR", "missing or invalid 'method'")

    params = obj.get("params")
    if params is None:
        params = {}
    if not isinstance(params, dict):
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", "'params' must be an object if present"
        )

    request_id = obj.get("id")
    if request_id is not None and not isinstance(request_id, int | str):
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", "'id' must be int, string, or null"
        )

    return request_id, method, params


def encode_response(request_id: int | str | None, result: Any) -> bytes:
    payload: dict[str, Any] = {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "result": result,
    }
    return (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")


def decode_response(line: bytes) -> dict[str, Any]:
    try:
        obj = json.loads(line)
    except json.JSONDecodeError as exc:
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", f"invalid JSON: {exc}"
        ) from exc
    if not isinstance(obj, dict):
        raise PluginTransportError("PLUGIN_TRANSPORT_ERROR", "response must be a JSON object")
    if obj.get("jsonrpc") != JSONRPC_VERSION:
        raise PluginTransportError(
            "PLUGIN_TRANSPORT_ERROR", f"unsupported jsonrpc version: {obj.get('jsonrpc')!r}"
        )
    return obj


def encode_error(
    request_id: int | str | None,
    exc: BaseException,
    *,
    code: int | None = None,
    data: dict[str, Any] | None = None,
) -> bytes:
    resolved_code = code if code is not None else _error_code_for(exc)
    error_obj: dict[str, Any] = {
        "code": resolved_code,
        "message": str(exc),
    }
    if isinstance(exc, PluginError):
        error_obj["data"] = {"code": exc.code, "message": exc.message}
    if data is not None:
        error_obj.setdefault("data", {}).update(data)
    payload = {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "error": error_obj,
    }
    return (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")


__all__ = [
    "ERROR_CODE_CAPABILITY",
    "ERROR_CODE_DISPATCH",
    "ERROR_CODE_INTERNAL",
    "ERROR_CODE_INVALID_REQUEST",
    "ERROR_CODE_METHOD_NOT_FOUND",
    "ERROR_CODE_PARSE",
    "ERROR_CODE_PLUGIN",
    "ERROR_CODE_REGISTRATION",
    "ERROR_CODE_TRANSPORT",
    "JSONRPC_VERSION",
    "decode_request",
    "decode_response",
    "encode_error",
    "encode_request",
    "encode_response",
]

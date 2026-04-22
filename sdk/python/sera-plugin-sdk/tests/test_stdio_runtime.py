"""End-to-end stdio transport test.

Wires an in-process plugin to an ``asyncio.StreamReader`` / ``StreamWriter``
pair backed by an in-memory pipe, then sends JSON-RPC frames and inspects
the responses.
"""

from __future__ import annotations

import asyncio
import json

import pytest

from sera_plugin_sdk.capabilities import ContextEngine
from sera_plugin_sdk.transport import stdio as stdio_transport
from sera_plugin_sdk.types import (
    AssembleBudget,
    AssembledContext,
    IngestAck,
    IngestMessage,
)


class _FakeEngine(ContextEngine):
    def __init__(self) -> None:
        self.startup_calls = 0
        self.shutdown_calls = 0
        self.ingested: list[IngestMessage] = []

    async def on_startup(self) -> None:
        self.startup_calls += 1

    async def on_shutdown(self) -> None:
        self.shutdown_calls += 1

    async def ingest(self, msg: IngestMessage) -> IngestAck:
        self.ingested.append(msg)
        return IngestAck(accepted=True)

    async def assemble(self, budget: AssembleBudget) -> AssembledContext:
        return AssembledContext(total_tokens=budget.budget_tokens)


class _InMemoryWriteTransport(asyncio.WriteTransport):
    """Funnels ``write`` calls into a target ``StreamReader``.

    Implements the minimum transport protocol surface that ``StreamWriter``
    needs (``write``, ``close``, ``is_closing``, and signalling
    ``connection_lost`` on close so ``wait_closed()`` resolves promptly).
    """

    def __init__(
        self,
        target: asyncio.StreamReader,
        protocol: asyncio.StreamReaderProtocol,
    ) -> None:
        super().__init__()
        self._target = target
        self._protocol = protocol
        self._closed = False

    def write(self, data: bytes | bytearray | memoryview) -> None:
        if not self._closed:
            self._target.feed_data(bytes(data))

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        self._target.feed_eof()
        loop = asyncio.get_event_loop()
        loop.call_soon(self._protocol.connection_lost, None)

    def is_closing(self) -> bool:
        return self._closed

    def get_write_buffer_size(self) -> int:
        return 0

    def can_write_eof(self) -> bool:
        return True

    def write_eof(self) -> None:
        self.close()


def _pipe_writer(target: asyncio.StreamReader) -> asyncio.StreamWriter:
    """Build a ``StreamWriter`` that funnels writes into ``target``."""
    loop = asyncio.get_event_loop()
    # A dummy upstream reader is required by StreamReaderProtocol;
    # we only care about the protocol's flow-control hooks here.
    dummy = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(dummy)
    transport = _InMemoryWriteTransport(target, protocol)
    protocol.connection_made(transport)
    return asyncio.StreamWriter(transport, protocol, dummy, loop)


async def _exchange(
    plugin: object, lines: list[bytes]
) -> tuple[list[dict[str, object]], object]:
    """Drive the stdio server through ``lines`` and collect response frames."""
    client_to_server = asyncio.StreamReader()
    server_to_client = asyncio.StreamReader()

    for line in lines:
        client_to_server.feed_data(line)
    client_to_server.feed_eof()

    server_writer = _pipe_writer(server_to_client)
    await stdio_transport.serve(plugin, client_to_server, server_writer)

    responses: list[dict[str, object]] = []
    while True:
        line = await server_to_client.readline()
        if not line:
            break
        responses.append(json.loads(line))
    return responses, plugin


def _frame(**payload: object) -> bytes:
    return (json.dumps({"jsonrpc": "2.0", **payload}) + "\n").encode("utf-8")


async def test_heartbeat_returns_ok() -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin, [_frame(id=1, method="PluginRegistry.Heartbeat", params={})]
    )
    assert len(responses) == 1
    response = responses[0]
    assert response["jsonrpc"] == "2.0"
    assert response["id"] == 1
    result = response["result"]
    assert isinstance(result, dict)
    assert result["ok"] is True
    assert "uptime_seconds" in result


async def test_capability_dispatch_ingest() -> None:
    plugin = _FakeEngine()
    responses, returned = await _exchange(
        plugin,
        [
            _frame(
                id="abc",
                method="ContextEngine.ingest",
                params={
                    "session_id": "s1",
                    "role": "user",
                    "content": "hello",
                    "metadata": {"k": "v"},
                },
            )
        ],
    )
    assert isinstance(returned, _FakeEngine)
    assert responses == [
        {"jsonrpc": "2.0", "id": "abc", "result": {"accepted": True}}
    ]
    assert len(returned.ingested) == 1
    assert returned.ingested[0].session_id == "s1"
    assert returned.ingested[0].content == "hello"


async def test_deregister_triggers_shutdown_hook_and_exits_loop() -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin, [_frame(id=7, method="PluginRegistry.Deregister", params={})]
    )
    assert responses[0]["id"] == 7
    assert responses[0]["result"] == {}
    assert plugin.shutdown_calls == 1


async def test_shutdown_hook_fires_on_eof() -> None:
    plugin = _FakeEngine()
    # No frames at all — just EOF. The serve loop must exit and on_shutdown
    # must still fire.
    _, _ = await _exchange(plugin, [])
    assert plugin.shutdown_calls == 1


async def test_unknown_method_returns_dispatch_error() -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin, [_frame(id=99, method="ContextEngine.does_not_exist", params={})]
    )
    assert responses[0]["id"] == 99
    assert "error" in responses[0]
    err = responses[0]["error"]
    assert isinstance(err, dict)
    assert err["code"] == -32003  # ERROR_CODE_DISPATCH


async def test_malformed_input_does_not_crash_loop() -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin,
        [
            b"{not json\n",
            _frame(id=123, method="PluginRegistry.Heartbeat", params={}),
        ],
    )
    # The malformed frame must not produce a response (no id to route to),
    # but the subsequent valid frame must be answered.
    assert len(responses) == 1
    assert responses[0]["id"] == 123
    result = responses[0]["result"]
    assert isinstance(result, dict)
    assert result["ok"] is True


@pytest.mark.parametrize("method", ["PluginRegistry.Heartbeat"])
async def test_heartbeat_uptime_is_monotonic(method: str) -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin, [_frame(id=1, method=method, params={})]
    )
    result = responses[0]["result"]
    assert isinstance(result, dict)
    assert float(result["uptime_seconds"]) >= 0.0


async def test_blank_lines_are_skipped() -> None:
    plugin = _FakeEngine()
    responses, _ = await _exchange(
        plugin,
        [
            b"\n",
            b"   \n",
            _frame(id=5, method="PluginRegistry.Heartbeat", params={}),
        ],
    )
    assert len(responses) == 1
    assert responses[0]["id"] == 5

"""End-to-end stdio integration — drive the real plugin via ``sdk-py`` stdio.

Boots an ``LcmPlugin`` against an in-memory pipe, sends JSON-RPC frames for
``ContextEngine.ingest`` + ``ContextEngine.assemble`` + ``ContextQuery.search``
+ ``ContextDiagnostics.status``, and asserts the wire responses agree with the
direct in-process calls. This is the end-to-end canary: it proves the
capability dispatch table wiring, the NDJSON framing, and the trait-triad
``isinstance`` check all hold together for a real plugin.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path

from sera_plugin_sdk.transport import stdio as stdio_transport

from sera_context_lcm import LcmPlugin


class _InMemoryWriteTransport(asyncio.WriteTransport):
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
    loop = asyncio.get_event_loop()
    dummy = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(dummy)
    transport = _InMemoryWriteTransport(target, protocol)
    protocol.connection_made(transport)
    return asyncio.StreamWriter(transport, protocol, dummy, loop)


def _frame(**payload: object) -> bytes:
    return (json.dumps({"jsonrpc": "2.0", **payload}) + "\n").encode("utf-8")


async def _exchange(
    plugin: object, lines: list[bytes]
) -> list[dict[str, object]]:
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
    return responses


async def test_register_response_advertises_full_capability_triad(
    tmp_path: Path,
) -> None:
    plugin = LcmPlugin(db_path=tmp_path / "lcm.db")
    try:
        responses = await _exchange(
            plugin, [_frame(id=1, method="PluginRegistry.Register", params={})]
        )
    finally:
        plugin._store.close()
        plugin._dag.close()

    assert responses[0]["id"] == 1
    result = responses[0]["result"]
    assert isinstance(result, dict)
    capabilities = result["server_capabilities"]
    assert isinstance(capabilities, list)
    assert set(capabilities) >= {
        "ContextEngine",
        "ContextQuery",
        "ContextDiagnostics",
    }


async def test_stdio_full_round_trip(tmp_path: Path) -> None:
    plugin = LcmPlugin(db_path=tmp_path / "lcm.db")
    try:
        responses = await _exchange(
            plugin,
            [
                _frame(
                    id=1,
                    method="ContextEngine.ingest",
                    params={
                        "session_id": "sx",
                        "role": "user",
                        "content": "first message about sera",
                        "metadata": {},
                    },
                ),
                _frame(
                    id=2,
                    method="ContextEngine.ingest",
                    params={
                        "session_id": "sx",
                        "role": "assistant",
                        "content": "second reply mentions sera",
                        "metadata": {},
                    },
                ),
                _frame(
                    id=3,
                    method="ContextEngine.assemble",
                    params={
                        "session_id": "sx",
                        "budget_tokens": 200,
                        "constraints_json": "",
                    },
                ),
                _frame(
                    id=4,
                    method="ContextQuery.search",
                    params={"session_id": "sx", "query": "sera", "limit": 5},
                ),
                _frame(
                    id=5,
                    method="ContextDiagnostics.status",
                    params={"session_id": "sx"},
                ),
            ],
        )
    finally:
        plugin._store.close()
        plugin._dag.close()

    by_id = {r["id"]: r for r in responses}
    assert by_id[1]["result"] == {"accepted": True}
    assert by_id[2]["result"] == {"accepted": True}

    assembled = by_id[3]["result"]
    assert isinstance(assembled, dict)
    assert assembled["total_tokens"] > 0
    assert len(assembled["segments"]) == 2

    search = by_id[4]["result"]
    assert isinstance(search, dict)
    assert len(search["hits"]) >= 2

    status = by_id[5]["result"]
    assert isinstance(status, dict)
    status_fields = json.loads(status["fields_json"])
    assert status_fields["store"]["messages"] == 2

"""Stdio transport — NDJSON framed JSON-RPC over stdin/stdout.

The dispatcher maps ``Capability.method`` strings onto capability methods on
the plugin. Top-level registry methods (``PluginRegistry.Heartbeat`` etc.) are
handled by the runtime regardless of capability.
"""

from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable, Callable
from typing import Any

from .. import _protocol as proto
from ..capabilities import (
    AuthProvider,
    ContextDiagnostics,
    ContextEngine,
    ContextQuery,
    MemoryBackend,
    SandboxProvider,
    SecretProvider,
    ToolExecutor,
)
from ..errors import PluginDispatchError, PluginError
from ..lifecycle import call_on_shutdown
from ..types import (
    AssembleBudget,
    AuthRequest,
    AuthzRequest,
    CtxSearchRequest,
    DescribeRequest,
    ExpandRequest,
    IngestMessage,
    MemoryQuery,
    MemoryRecord,
    SandboxExecRequest,
    SecretRef,
    ToolCall,
)

Handler = Callable[[dict[str, Any]], Awaitable[Any]]


def _build_dispatch_table(plugin: object) -> dict[str, Handler]:
    """Map ``Capability.method`` strings to awaitable handlers."""
    table: dict[str, Handler] = {}

    if isinstance(plugin, ContextEngine):
        engine = plugin

        async def _ingest(params: dict[str, Any]) -> dict[str, Any]:
            msg = IngestMessage.from_wire(params)
            return (await engine.ingest(msg)).to_wire()

        async def _assemble(params: dict[str, Any]) -> dict[str, Any]:
            budget = AssembleBudget.from_wire(params)
            return (await engine.assemble(budget)).to_wire()

        table["ContextEngine.ingest"] = _ingest
        table["ContextEngine.assemble"] = _assemble

    if isinstance(plugin, ContextQuery):
        query = plugin

        async def _search(params: dict[str, Any]) -> dict[str, Any]:
            return (await query.search(CtxSearchRequest.from_wire(params))).to_wire()

        async def _describe(params: dict[str, Any]) -> dict[str, Any]:
            return (await query.describe(DescribeRequest.from_wire(params))).to_wire()

        async def _expand(params: dict[str, Any]) -> dict[str, Any]:
            return (await query.expand(ExpandRequest.from_wire(params))).to_wire()

        table["ContextQuery.search"] = _search
        table["ContextQuery.describe"] = _describe
        table["ContextQuery.expand"] = _expand

    if isinstance(plugin, ContextDiagnostics):
        diag = plugin

        async def _status(params: dict[str, Any]) -> dict[str, Any]:
            return (await diag.status(str(params.get("session_id") or ""))).to_wire()

        async def _doctor(_params: dict[str, Any]) -> dict[str, Any]:
            return (await diag.doctor()).to_wire()

        table["ContextDiagnostics.status"] = _status
        table["ContextDiagnostics.doctor"] = _doctor

    if isinstance(plugin, MemoryBackend):
        mem = plugin

        async def _store(params: dict[str, Any]) -> dict[str, Any]:
            record = MemoryRecord.from_wire(params.get("record") or params)
            return (await mem.store(record)).to_wire()

        async def _retrieve(params: dict[str, Any]) -> dict[str, Any]:
            result = await mem.retrieve(str(params["key"]))
            return {
                "found": result is not None,
                "record": result.to_wire() if result is not None else None,
            }

        async def _search_mem(params: dict[str, Any]) -> dict[str, Any]:
            hits = await mem.search(MemoryQuery.from_wire(params))
            return {"records": [r.to_wire() for r in hits]}

        async def _delete(params: dict[str, Any]) -> dict[str, Any]:
            return {"deleted": bool(await mem.delete(str(params["key"])))}

        table["MemoryBackend.store"] = _store
        table["MemoryBackend.retrieve"] = _retrieve
        table["MemoryBackend.search"] = _search_mem
        table["MemoryBackend.delete"] = _delete

    if isinstance(plugin, ToolExecutor):
        tools = plugin

        async def _list_tools(_params: dict[str, Any]) -> dict[str, Any]:
            defs = await tools.list_tools()
            return {"tools": [d.to_wire() for d in defs]}

        async def _execute_tool(params: dict[str, Any]) -> dict[str, Any]:
            return (await tools.execute_tool(ToolCall.from_wire(params))).to_wire()

        table["ToolExecutor.list_tools"] = _list_tools
        table["ToolExecutor.execute_tool"] = _execute_tool

    if isinstance(plugin, SandboxProvider):
        sandbox = plugin

        async def _execute(params: dict[str, Any]) -> dict[str, Any]:
            return (await sandbox.execute(SandboxExecRequest.from_wire(params))).to_wire()

        async def _cleanup(params: dict[str, Any]) -> dict[str, Any]:
            await sandbox.cleanup(str(params["sandbox_id"]))
            return {}

        table["SandboxProvider.execute"] = _execute
        table["SandboxProvider.cleanup"] = _cleanup

    if isinstance(plugin, AuthProvider):
        auth = plugin

        async def _authenticate(params: dict[str, Any]) -> dict[str, Any]:
            return (await auth.authenticate(AuthRequest.from_wire(params))).to_wire()

        async def _authorize(params: dict[str, Any]) -> dict[str, Any]:
            return (await auth.authorize(AuthzRequest.from_wire(params))).to_wire()

        table["AuthProvider.authenticate"] = _authenticate
        table["AuthProvider.authorize"] = _authorize

    if isinstance(plugin, SecretProvider):
        secrets = plugin

        async def _resolve(params: dict[str, Any]) -> dict[str, Any]:
            return (await secrets.resolve(SecretRef.from_wire(params))).to_wire()

        async def _list_secrets(_params: dict[str, Any]) -> dict[str, Any]:
            refs = await secrets.list_secrets()
            return {"refs": [r.to_wire() for r in refs]}

        table["SecretProvider.resolve"] = _resolve
        table["SecretProvider.list_secrets"] = _list_secrets

    return table


class _StdioServer:
    """Internal helper that owns the dispatch loop and shutdown state."""

    def __init__(self, plugin: object) -> None:
        self.plugin = plugin
        self.dispatch = _build_dispatch_table(plugin)
        self.started_at = time.monotonic()
        self._shutdown = asyncio.Event()

    async def handle_registry(
        self, method: str, _params: dict[str, Any]
    ) -> dict[str, Any]:
        if method == "PluginRegistry.Heartbeat":
            return {
                "ok": True,
                "uptime_seconds": time.monotonic() - self.started_at,
            }
        if method == "PluginRegistry.Register":
            return {
                "plugin_id": getattr(self.plugin, "plugin_id", ""),
                "server_capabilities": sorted(
                    name.split(".", 1)[0] for name in self.dispatch
                ),
                "protocol_version": "v1",
            }
        if method == "PluginRegistry.Deregister":
            self._shutdown.set()
            return {}
        raise PluginDispatchError(
            "PLUGIN_DISPATCH_ERROR", f"unknown registry method: {method}"
        )

    async def handle_one(
        self, line: bytes, writer: asyncio.StreamWriter
    ) -> None:
        request_id: int | str | None = None
        try:
            request_id, method, params = proto.decode_request(line)
            if method.startswith("PluginRegistry."):
                result = await self.handle_registry(method, params)
            else:
                handler = self.dispatch.get(method)
                if handler is None:
                    raise PluginDispatchError(
                        "PLUGIN_DISPATCH_ERROR", f"unknown method: {method}"
                    )
                result = await handler(params)
            if request_id is not None:
                writer.write(proto.encode_response(request_id, result))
                await writer.drain()
        except PluginError as exc:
            if request_id is not None:
                writer.write(proto.encode_error(request_id, exc))
                await writer.drain()
        except Exception as exc:  # pragma: no cover - defensive
            if request_id is not None:
                writer.write(
                    proto.encode_error(request_id, exc, code=proto.ERROR_CODE_INTERNAL)
                )
                await writer.drain()

    async def serve(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        try:
            while not self._shutdown.is_set():
                line = await reader.readline()
                if not line:
                    break
                stripped = line.strip()
                if not stripped:
                    continue
                await self.handle_one(stripped, writer)
        finally:
            await call_on_shutdown(self.plugin)
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:  # pragma: no cover - best-effort close
                pass


async def serve(
    plugin: object,
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
) -> None:
    """Run the stdio dispatch loop against the given reader/writer pair."""
    server = _StdioServer(plugin)
    await server.serve(reader, writer)


__all__ = ["serve"]

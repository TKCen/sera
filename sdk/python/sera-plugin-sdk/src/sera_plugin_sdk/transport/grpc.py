"""gRPC transport scaffold.

This is a minimal scaffold that starts an ``grpc.aio`` server and wires a
``PluginRegistry`` servicer. Capability servicers raise ``UNIMPLEMENTED`` —
the Rust gateway already knows how to treat that as "capability not served
by this plugin." The intent is that the server starts, accepts Registry
calls, and lets the operator verify connectivity. Full capability dispatch
is a follow-up bead.

Imports of the generated protobuf modules are lazy (and typed as ``Any``) so
the module stays importable and type-checkable before the Hatch build hook
has populated ``sera_plugin_sdk._generated``.
"""

from __future__ import annotations

import importlib
import time
from typing import Any


class GrpcServer:
    """Minimal async gRPC server wrapper.

    The full capability-to-proto wiring depends on generated protobuf modules
    that are populated by the Hatch build hook; if those modules are absent
    this wrapper raises ``RuntimeError`` on :meth:`start`.
    """

    def __init__(self, plugin: object, bind: str = "0.0.0.0:0") -> None:
        self.plugin = plugin
        self.bind = bind
        self._server: Any = None
        self._actual_port: int | None = None
        self._started_at: float = 0.0

    async def start(self) -> None:
        try:
            grpc_aio: Any = importlib.import_module("grpc.aio")
        except ImportError as exc:  # pragma: no cover - grpcio is a hard dep
            raise RuntimeError(
                "grpcio is not installed; install sera-plugin-sdk with the "
                "gRPC extras to use the gRPC transport"
            ) from exc

        try:
            registry_pb2: Any = importlib.import_module(
                "sera_plugin_sdk._generated.registry_pb2"
            )
            registry_pb2_grpc: Any = importlib.import_module(
                "sera_plugin_sdk._generated.registry_pb2_grpc"
            )
        except ImportError as exc:
            raise RuntimeError(
                "generated protobuf modules missing; run `hatch build` or "
                "install the SDK via a wheel so the build hook can run protoc"
            ) from exc

        plugin = self.plugin
        self._started_at = time.monotonic()

        class _RegistryServicer(registry_pb2_grpc.PluginRegistryServicer):  # type: ignore[misc]
            async def Register(self, _request: Any, _context: Any) -> Any:
                ack = registry_pb2.RegistrationAck()
                ack.plugin_id = getattr(plugin, "plugin_id", "")
                ack.protocol_version = "v1"
                return ack

            async def Heartbeat(self, _request: Any, _context: Any) -> Any:
                ack = registry_pb2.HeartbeatAck()
                ack.ok = True
                return ack

            async def Deregister(self, _request: Any, _context: Any) -> Any:
                empty_pb2: Any = importlib.import_module("google.protobuf.empty_pb2")
                return empty_pb2.Empty()

        server = grpc_aio.server()
        registry_pb2_grpc.add_PluginRegistryServicer_to_server(
            _RegistryServicer(), server
        )
        self._actual_port = server.add_insecure_port(self.bind)
        await server.start()
        self._server = server

    async def stop(self, grace: float = 5.0) -> None:
        if self._server is not None:
            await self._server.stop(grace)
            self._server = None

    async def wait_closed(self) -> None:
        if self._server is not None:
            await self._server.wait_for_termination()

    @property
    def port(self) -> int | None:
        return self._actual_port


__all__ = ["GrpcServer"]

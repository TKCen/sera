"""Top-level plugin runtime entrypoints.

``run_stdio_plugin`` wires the plugin to ``sys.stdin`` / ``sys.stdout`` with
NDJSON framing. ``run_grpc_plugin`` starts a gRPC server bound to ``bind``.
Both call the optional ``on_startup`` hook before serving and ``on_shutdown``
on graceful termination.
"""

from __future__ import annotations

import asyncio
import contextlib
import signal
import sys

from .lifecycle import call_on_shutdown, call_on_startup
from .transport import stdio as stdio_transport


async def _stdin_stdout_streams() -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
    loop = asyncio.get_running_loop()
    reader = asyncio.StreamReader()
    await loop.connect_read_pipe(
        lambda: asyncio.StreamReaderProtocol(reader), sys.stdin
    )
    transport, protocol = await loop.connect_write_pipe(
        asyncio.streams.FlowControlMixin, sys.stdout
    )
    writer = asyncio.StreamWriter(transport, protocol, reader, loop)
    return reader, writer


def _install_signal_handlers(stop: asyncio.Event) -> None:
    loop = asyncio.get_running_loop()
    for signame in ("SIGTERM", "SIGINT"):
        sig = getattr(signal, signame, None)
        if sig is None:
            continue
        try:
            loop.add_signal_handler(sig, stop.set)
        except NotImplementedError:  # pragma: no cover - Windows
            signal.signal(sig, lambda *_args: stop.set())


async def _run_stdio(plugin: object) -> None:
    await call_on_startup(plugin)
    reader, writer = await _stdin_stdout_streams()
    stop = asyncio.Event()
    _install_signal_handlers(stop)

    serve_task = asyncio.create_task(stdio_transport.serve(plugin, reader, writer))
    stop_task = asyncio.create_task(stop.wait())

    done, pending = await asyncio.wait(
        {serve_task, stop_task}, return_when=asyncio.FIRST_COMPLETED
    )
    for task in pending:
        task.cancel()
        with contextlib.suppress(asyncio.CancelledError, Exception):
            await task
    for task in done:
        exc = task.exception()
        if exc is not None:
            raise exc


def run_stdio_plugin(plugin: object) -> None:
    """Entrypoint for stdio plugins. Blocks until the gateway deregisters."""
    asyncio.run(_run_stdio(plugin))


async def _run_grpc(plugin: object, bind: str) -> None:
    from .transport.grpc import GrpcServer

    server = GrpcServer(plugin, bind=bind)
    await call_on_startup(plugin)
    await server.start()
    stop = asyncio.Event()
    _install_signal_handlers(stop)
    try:
        await stop.wait()
    finally:
        await server.stop()
        await call_on_shutdown(plugin)


def run_grpc_plugin(plugin: object, bind: str = "0.0.0.0:0") -> None:
    """Entrypoint for gRPC plugins. Blocks until the server is signalled."""
    asyncio.run(_run_grpc(plugin, bind))


__all__ = ["run_grpc_plugin", "run_stdio_plugin"]

# sera-plugin-sdk

Python SDK for authoring SERA plugins. See
[SPEC-plugins](../../../docs/plan/specs/SPEC-plugins.md) §5 for the design record.

## What it is

A thin ergonomic wrapper over SERA's plugin wire contract. Plugin authors
subclass the capability ABCs they implement, wire the plugin into the stdio or
gRPC runtime, and ship it. The SDK owns transport framing, heartbeats, the
JSON-RPC codec, and graceful shutdown — the plugin owns the domain logic.

## Who it is for

- Developers building custom SERA backends (`MemoryBackend`, `ContextEngine`,
  `ToolExecutor`, `SandboxProvider`, `AuthProvider`, `SecretProvider`).
- Enterprise integrators wrapping proprietary services behind the SERA plugin
  contract.

## Minimal example

```python
# sera_context_lcm/__main__.py
from sera_plugin_sdk import (
    run_stdio_plugin,
    ContextEngine,
    ContextQuery,
    ContextDiagnostics,
)


class LcmPlugin(ContextEngine, ContextQuery, ContextDiagnostics):
    async def ingest(self, msg): ...
    async def assemble(self, budget): ...
    async def search(self, req): ...
    async def describe(self, req): ...
    async def expand(self, req): ...
    async def status(self, session_id): ...
    async def doctor(self): ...


if __name__ == "__main__":
    run_stdio_plugin(LcmPlugin())
```

## Install and test

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install -e '.[dev]'
python -m pytest -q
ruff check src tests
mypy src
hatch build
```

`hatch build` invokes `grpc_tools.protoc` against `rust/proto/plugin/*.proto`
and writes generated modules into `src/sera_plugin_sdk/_generated/`. The build
hook gracefully no-ops (with a warning) when the proto directory is absent, so
`pip install -e .` works in a standalone checkout.

## License

Apache-2.0. See `LICENSE`.

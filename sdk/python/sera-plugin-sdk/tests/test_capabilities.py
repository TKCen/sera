"""Tests for capability ABCs."""

from __future__ import annotations

import pytest

from sera_plugin_sdk.capabilities import (
    Capability,
    ContextDiagnostics,
    ContextEngine,
    ContextQuery,
    MemoryBackend,
)
from sera_plugin_sdk.types import (
    AssembleBudget,
    AssembledContext,
    CtxSearchRequest,
    CtxSearchResponse,
    DescribeRequest,
    DescribeResponse,
    DoctorReport,
    ExpandRequest,
    ExpandResponse,
    IngestAck,
    IngestMessage,
    MemoryQuery,
    MemoryRecord,
    StatusResponse,
    StoreAck,
)


def test_memory_backend_cannot_be_instantiated_abstract() -> None:
    with pytest.raises(TypeError):
        MemoryBackend()  # type: ignore[abstract]


def test_context_engine_cannot_be_instantiated_abstract() -> None:
    with pytest.raises(TypeError):
        ContextEngine()  # type: ignore[abstract]


def test_partial_implementation_still_abstract() -> None:
    class Partial(MemoryBackend):
        async def store(self, record: MemoryRecord) -> StoreAck:
            return StoreAck(key=record.key)

    with pytest.raises(TypeError):
        Partial()  # type: ignore[abstract]


def test_memory_backend_full_impl_instantiates() -> None:
    class InMem(MemoryBackend):
        def __init__(self) -> None:
            self.store_map: dict[str, MemoryRecord] = {}

        async def store(self, record: MemoryRecord) -> StoreAck:
            self.store_map[record.key] = record
            return StoreAck(key=record.key)

        async def retrieve(self, key: str) -> MemoryRecord | None:
            return self.store_map.get(key)

        async def search(self, query: MemoryQuery) -> list[MemoryRecord]:
            return list(self.store_map.values())[: query.limit]

        async def delete(self, key: str) -> bool:
            return self.store_map.pop(key, None) is not None

    backend = InMem()
    assert isinstance(backend, MemoryBackend)
    assert isinstance(backend, Capability)


def test_multi_inheritance_context_triad() -> None:
    class LcmLike(ContextEngine, ContextQuery, ContextDiagnostics):
        async def ingest(self, msg: IngestMessage) -> IngestAck:
            return IngestAck(accepted=True)

        async def assemble(self, budget: AssembleBudget) -> AssembledContext:
            return AssembledContext()

        async def search(self, req: CtxSearchRequest) -> CtxSearchResponse:
            return CtxSearchResponse()

        async def describe(self, req: DescribeRequest) -> DescribeResponse:
            return DescribeResponse(node_id=req.node_id)

        async def expand(self, req: ExpandRequest) -> ExpandResponse:
            return ExpandResponse(node_id=req.node_id)

        async def status(self, session_id: str) -> StatusResponse:
            return StatusResponse(fields_json="{}")

        async def doctor(self) -> DoctorReport:
            return DoctorReport()

    plugin = LcmLike()
    assert isinstance(plugin, ContextEngine)
    assert isinstance(plugin, ContextQuery)
    assert isinstance(plugin, ContextDiagnostics)
    assert isinstance(plugin, Capability)

"""ContextEngine / ContextQuery / ContextDiagnostics capability ABCs.

Mirrors the three services in ``rust/proto/plugin/context_engine.proto``. A
plugin that implements the full context-engine contract inherits from all
three; engines that only provide the core seam inherit just ``ContextEngine``.
"""

from __future__ import annotations

import abc

from ..types import (
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
    StatusResponse,
)
from .base import Capability


class ContextEngine(Capability, abc.ABC):
    """Core context-engine seam. Required for PLUGIN_CAPABILITY_CONTEXT_ENGINE."""

    @abc.abstractmethod
    async def ingest(self, msg: IngestMessage) -> IngestAck: ...

    @abc.abstractmethod
    async def assemble(self, budget: AssembleBudget) -> AssembledContext: ...


class ContextQuery(Capability, abc.ABC):
    """Drill-tool surface — implemented by engines with an addressable store."""

    @abc.abstractmethod
    async def search(self, req: CtxSearchRequest) -> CtxSearchResponse: ...

    @abc.abstractmethod
    async def describe(self, req: DescribeRequest) -> DescribeResponse: ...

    @abc.abstractmethod
    async def expand(self, req: ExpandRequest) -> ExpandResponse: ...


class ContextDiagnostics(Capability, abc.ABC):
    """Health / introspection surface. status() and doctor()."""

    @abc.abstractmethod
    async def status(self, session_id: str) -> StatusResponse: ...

    @abc.abstractmethod
    async def doctor(self) -> DoctorReport: ...


__all__ = ["ContextDiagnostics", "ContextEngine", "ContextQuery"]

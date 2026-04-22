"""MemoryBackend capability ABC."""

from __future__ import annotations

import abc

from ..types import MemoryQuery, MemoryRecord, StoreAck
from .base import Capability


class MemoryBackend(Capability, abc.ABC):
    @abc.abstractmethod
    async def store(self, record: MemoryRecord) -> StoreAck: ...

    @abc.abstractmethod
    async def retrieve(self, key: str) -> MemoryRecord | None: ...

    @abc.abstractmethod
    async def search(self, query: MemoryQuery) -> list[MemoryRecord]: ...

    @abc.abstractmethod
    async def delete(self, key: str) -> bool: ...


__all__ = ["MemoryBackend"]

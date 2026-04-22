"""SandboxProvider capability ABC."""

from __future__ import annotations

import abc

from ..types import SandboxExecRequest, SandboxExecResult
from .base import Capability


class SandboxProvider(Capability, abc.ABC):
    @abc.abstractmethod
    async def execute(self, req: SandboxExecRequest) -> SandboxExecResult: ...

    @abc.abstractmethod
    async def cleanup(self, sandbox_id: str) -> None: ...


__all__ = ["SandboxProvider"]

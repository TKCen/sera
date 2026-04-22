"""ToolExecutor capability ABC."""

from __future__ import annotations

import abc

from ..types import ToolCall, ToolDefinition, ToolResult
from .base import Capability


class ToolExecutor(Capability, abc.ABC):
    @abc.abstractmethod
    async def list_tools(self) -> list[ToolDefinition]: ...

    @abc.abstractmethod
    async def execute_tool(self, call: ToolCall) -> ToolResult: ...


__all__ = ["ToolExecutor"]

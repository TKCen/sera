"""Capability ABCs. A plugin inherits from one or more of these."""

from __future__ import annotations

from .auth import AuthProvider
from .base import Capability
from .context import ContextDiagnostics, ContextEngine, ContextQuery
from .memory import MemoryBackend
from .sandbox import SandboxProvider
from .secrets import SecretProvider
from .tools import ToolExecutor

__all__ = [
    "AuthProvider",
    "Capability",
    "ContextDiagnostics",
    "ContextEngine",
    "ContextQuery",
    "MemoryBackend",
    "SandboxProvider",
    "SecretProvider",
    "ToolExecutor",
]

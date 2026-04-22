"""sera-plugin-sdk — Python SDK for authoring SERA plugins.

Public API:

- Capability ABCs: :class:`ContextEngine`, :class:`ContextQuery`,
  :class:`ContextDiagnostics`, :class:`MemoryBackend`, :class:`ToolExecutor`,
  :class:`SandboxProvider`, :class:`AuthProvider`, :class:`SecretProvider`.
- Runtime entrypoints: :func:`run_stdio_plugin`, :func:`run_grpc_plugin`.
- Error hierarchy: :class:`PluginError` and subclasses.
- Wire-neutral dataclasses in :mod:`sera_plugin_sdk.types`.
"""

from __future__ import annotations

from .capabilities import (
    AuthProvider,
    Capability,
    ContextDiagnostics,
    ContextEngine,
    ContextQuery,
    MemoryBackend,
    SandboxProvider,
    SecretProvider,
    ToolExecutor,
)
from .errors import (
    PluginCapabilityError,
    PluginDispatchError,
    PluginError,
    PluginRegistrationError,
    PluginTransportError,
)
from .runtime import run_grpc_plugin, run_stdio_plugin

__version__ = "0.1.0"

__all__ = [
    "AuthProvider",
    "Capability",
    "ContextDiagnostics",
    "ContextEngine",
    "ContextQuery",
    "MemoryBackend",
    "PluginCapabilityError",
    "PluginDispatchError",
    "PluginError",
    "PluginRegistrationError",
    "PluginTransportError",
    "SandboxProvider",
    "SecretProvider",
    "ToolExecutor",
    "__version__",
    "run_grpc_plugin",
    "run_stdio_plugin",
]

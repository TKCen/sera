"""Optional lifecycle hook detection.

Plugin authors may define ``async def on_startup(self)`` / ``async def
on_shutdown(self)`` on their plugin class. The runtime calls the hook if
present, no-ops otherwise, and rejects sync definitions explicitly — an async
runtime cannot safely run a blocking startup hook.
"""

from __future__ import annotations

import inspect

from .errors import PluginError

LIFECYCLE_ERROR_CODE = "PLUGIN_LIFECYCLE_NOT_ASYNC"


async def call_on_startup(plugin: object) -> None:
    """Invoke ``plugin.on_startup()`` if defined. Must be async."""
    await _invoke_hook(plugin, "on_startup")


async def call_on_shutdown(plugin: object) -> None:
    """Invoke ``plugin.on_shutdown()`` if defined. Must be async."""
    await _invoke_hook(plugin, "on_shutdown")


async def _invoke_hook(plugin: object, name: str) -> None:
    hook = getattr(plugin, name, None)
    if hook is None:
        return
    if not callable(hook):
        raise PluginError(LIFECYCLE_ERROR_CODE, f"{name} must be callable")
    if not inspect.iscoroutinefunction(hook):
        raise PluginError(LIFECYCLE_ERROR_CODE, f"{name} must be an async def")
    await hook()


__all__ = ["LIFECYCLE_ERROR_CODE", "call_on_shutdown", "call_on_startup"]

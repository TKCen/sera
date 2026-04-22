"""Tests for :mod:`sera_plugin_sdk.lifecycle`."""

from __future__ import annotations

import pytest

from sera_plugin_sdk.errors import PluginError
from sera_plugin_sdk.lifecycle import (
    LIFECYCLE_ERROR_CODE,
    call_on_shutdown,
    call_on_startup,
)


class WithAsyncHooks:
    def __init__(self) -> None:
        self.startup_calls = 0
        self.shutdown_calls = 0

    async def on_startup(self) -> None:
        self.startup_calls += 1

    async def on_shutdown(self) -> None:
        self.shutdown_calls += 1


class WithSyncStartup:
    # Intentionally sync to trigger rejection.
    def on_startup(self) -> None:
        return None


class NoHooks:
    pass


async def test_calls_async_on_startup_exactly_once() -> None:
    plugin = WithAsyncHooks()
    await call_on_startup(plugin)
    assert plugin.startup_calls == 1
    assert plugin.shutdown_calls == 0


async def test_calls_async_on_shutdown_exactly_once() -> None:
    plugin = WithAsyncHooks()
    await call_on_shutdown(plugin)
    assert plugin.shutdown_calls == 1
    assert plugin.startup_calls == 0


async def test_no_hooks_is_noop() -> None:
    plugin = NoHooks()
    await call_on_startup(plugin)
    await call_on_shutdown(plugin)


async def test_sync_hook_is_rejected() -> None:
    plugin = WithSyncStartup()
    with pytest.raises(PluginError) as excinfo:
        await call_on_startup(plugin)
    assert excinfo.value.code == LIFECYCLE_ERROR_CODE
    assert "on_startup" in excinfo.value.message

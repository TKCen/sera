"""Stdio entrypoint — ``python -m sera_context_lcm``.

Boots the LCM plugin and hands control to :func:`sera_plugin_sdk.run_stdio_plugin`
which drives the NDJSON JSON-RPC loop over stdin/stdout.
"""

from __future__ import annotations

from sera_plugin_sdk import run_stdio_plugin

from .plugin import LcmPlugin


def main() -> None:
    run_stdio_plugin(LcmPlugin())


if __name__ == "__main__":
    main()

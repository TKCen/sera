"""Plugin error hierarchy.

Every error exposes a stable ``code`` string and a human-readable ``message``.
The JSON-RPC codec (``sera_plugin_sdk._protocol``) maps each class to a
JSON-RPC error code so the gateway can classify failures without string
matching.
"""

from __future__ import annotations


class PluginError(Exception):
    """Base class for all SDK-visible plugin errors."""

    DEFAULT_CODE: str = "PLUGIN_ERROR"

    def __init__(self, code: str | None = None, message: str = "") -> None:
        self.code: str = code or self.DEFAULT_CODE
        self.message: str = message or self.code
        super().__init__(f"[{self.code}] {self.message}")


class PluginRegistrationError(PluginError):
    """Raised when registration or capability validation fails."""

    DEFAULT_CODE = "PLUGIN_REGISTRATION_ERROR"


class PluginTransportError(PluginError):
    """Raised for transport-layer failures (framing, I/O, codec)."""

    DEFAULT_CODE = "PLUGIN_TRANSPORT_ERROR"


class PluginDispatchError(PluginError):
    """Raised when a request cannot be routed to a capability method."""

    DEFAULT_CODE = "PLUGIN_DISPATCH_ERROR"


class PluginCapabilityError(PluginError):
    """Raised by capability implementations when they fail a request."""

    DEFAULT_CODE = "PLUGIN_CAPABILITY_ERROR"


__all__ = [
    "PluginCapabilityError",
    "PluginDispatchError",
    "PluginError",
    "PluginRegistrationError",
    "PluginTransportError",
]

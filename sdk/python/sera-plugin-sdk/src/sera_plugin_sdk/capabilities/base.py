"""Empty base class so consumers can ``isinstance(plugin, Capability)``."""

from __future__ import annotations


class Capability:
    """Marker base for every capability ABC.

    The runtime uses this to identify which capability contracts a plugin
    implements without enumerating every subclass. It is intentionally not
    an ``abc.ABC`` — the concrete capability classes (``MemoryBackend`` etc.)
    are the ABCs with abstract methods; this is just a shared supertype for
    ``isinstance`` checks.
    """


__all__ = ["Capability"]

"""sera-context-lcm — SERA ContextEngine plugin wrapping hermes-agent's LCM.

First out-of-process consumer of the Python plugin SDK. Implements the full
trait triad (``ContextEngine`` + ``ContextQuery`` + ``ContextDiagnostics``) by
binding directly to LCM's internal ``MessageStore`` / ``SummaryDAG`` /
``LCMEngine`` components — see
:mod:`sera_context_lcm.plugin` for the shape-translation rationale.
"""

from __future__ import annotations

from .plugin import LcmPlugin

__version__ = "0.1.0"

__all__ = ["LcmPlugin", "__version__"]

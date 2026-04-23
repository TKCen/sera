"""Vendored subset of hermes-agent's LCM engine.

Origin: ``hermes-agent/plugins/context_engine/lcm/`` (NousResearch/hermes-agent).

We vendor only the storage + search + tokenisation subset (``store.py``,
``dag.py``, ``tokens.py``, ``db_bootstrap.py``, ``search_query.py``) — the
pieces that have zero external hermes-agent dependencies. The compaction /
summarisation pipeline (``engine.py``, ``escalation.py``, ``extraction.py``,
``tools.py``) is **not** vendored because it depends on ``agent.*`` modules
from hermes-agent (auxiliary LLM client) that are out of scope for the
SERA trait surface — the shape translation happens in
:mod:`sera_context_lcm.plugin`.

Upstream tracking: the vendored files are local-only additions in the author's
hermes-agent checkout and are not published on any NousResearch/hermes-agent
branch at the time of writing (see bead sera-yf9r notes). If upstream lands
these files later, this subpackage can be replaced with a git submodule pin.

Do not edit the vendored files by hand — resync from the origin when the
upstream LCM code moves.
"""

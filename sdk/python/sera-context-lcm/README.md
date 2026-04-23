# sera-context-lcm

SERA `ContextEngine` plugin that wraps hermes-agent's **Lossless Context
Management (LCM)** engine. First out-of-process consumer of
[`sera-plugin-sdk`](../sera-plugin-sdk/) — demonstrates the stdio transport,
the `ContextEngine` capability, and the full trait triad
(`ContextEngine` + `ContextQuery` + `ContextDiagnostics`) described in
[SPEC-context-engine-pluggability](../../../docs/plan/specs/SPEC-context-engine-pluggability.md).

## What it does

LCM persists every message to SQLite, summarises older turns into a
depth-stratified DAG, and gives the agent drill tools (`lcm_grep`,
`lcm_describe`, `lcm_expand`, …) to reach back into compacted history. This
plugin binds LCM's internal `MessageStore` + `SummaryDAG` + `LCMEngine`
directly to SERA's `ContextEngine` / `ContextQuery` / `ContextDiagnostics`
trait surface per the SPEC §4 mapping table.

The plugin does **not** subclass hermes's public `ContextEngine` base class —
that class models one-shot `compress(messages, tokens)` compaction, which has
a fundamentally different shape from SERA's per-session stateful
`ingest / assemble / compact / maintain / describe` trait. The wire translation
happens in this adapter, not in the SDK ABC (see
`src/sera_context_lcm/plugin.py` docstring for detail).

## How hermes-agent code is pulled in

Only the minimal LCM storage + search subset
(`store.py`, `dag.py`, `tokens.py`, `db_bootstrap.py`, `search_query.py`)
is vendored in
[`src/sera_context_lcm/_lcm_core/`](src/sera_context_lcm/_lcm_core/__init__.py).
That subset has zero external `hermes-agent` imports — it depends only on the
standard library — so the plugin ships as a self-contained wheel with no
submodule dance.

We do **not** vendor the summarisation pipeline (`engine.py`,
`escalation.py`, `extraction.py`, `tools.py`) because those modules import
`agent.*` from hermes-agent (auxiliary LLM client, etc.), and the sdk-py
`ContextEngine` ABC models `ingest` / `assemble` / `compact` — the adapter
only needs storage + search surfaces. Compaction belongs behind the runtime
seam described in SPEC §2.1 and is not part of the sdk-py surface today.

The original LCM sources live in
`hermes-agent/plugins/context_engine/lcm/` and are not currently upstreamed
in the public NousResearch/hermes-agent repo. If that changes, the vendored
subdirectory can be replaced with a git submodule pin without touching the
adapter.

## Install and test

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install -e '../sera-plugin-sdk'
python -m pip install -e '.[dev]'
python -m pytest -q
ruff check src tests
```

## Running as a SERA plugin

The shipped `plugin.yaml` advertises the stdio transport with `ContextEngine`
capability. SERA's plugin registry spawns the subprocess and drives it over
NDJSON JSON-RPC per `SPEC-plugins` §8.

```bash
python -m sera_context_lcm  # starts the stdio server
```

## Fresh SQLite schema

This plugin writes to a **new** SQLite file (`~/.sera/plugins/lcm/lcm.db`
by default; override with `SERA_LCM_DATABASE_PATH`). There is no migration
from an existing hermes LCM database — if users need byte compatibility, that
becomes a separate bead.

## License

Apache-2.0. See `LICENSE`.

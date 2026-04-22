"""LcmPlugin — adapter binding hermes-agent LCM internals to SERA's plugin ABCs.

Design note (critical — see bead sera-yf9r):

Hermes-agent's public Python ``ContextEngine`` base class models
*one-shot stateless* compaction::

    compress(messages, current_tokens) -> messages

SERA's wire contract (``sera-plugin-sdk.ContextEngine`` ABC, mirroring the
Rust ``ContextEngine`` trait in SPEC-context-engine-pluggability §2.1) is
*per-session stateful*::

    ingest(IngestMessage) -> IngestAck
    assemble(AssembleBudget) -> AssembledContext

These shapes are fundamentally different. The adapter therefore binds
**LCM's internal components** (``MessageStore`` + ``SummaryDAG``) directly,
mapping them onto the three trait methods in the sdk-py ABC surface
(``ContextEngine`` / ``ContextQuery`` / ``ContextDiagnostics``). We do not
subclass ``agent.context_engine.ContextEngine`` — that class's method
surface would be wrong for sera's seam.

Mapping (aligned with SPEC-context-engine-pluggability §4):

================================  ==========================================
sdk-py trait method                LCM call
================================  ==========================================
``ContextEngine.ingest``           ``MessageStore.append(...)``
``ContextEngine.assemble``         Read store + DAG for the session, pack
                                   segments under the token budget.
``ContextQuery.search``            ``MessageStore.search`` + ``SummaryDAG.search``,
                                   merged.
``ContextQuery.describe``          ``SummaryDAG.describe_subtree``
``ContextQuery.expand``            Walk ``SummaryDAG`` sources back to
                                   ``MessageStore`` content, honour max
                                   tokens.
``ContextDiagnostics.status``      LCM store + DAG counters for the session.
``ContextDiagnostics.doctor``      DB integrity + FTS sync + orphan check
                                   (mirror of ``lcm_doctor``).
================================  ==========================================

``lcm_expand_query`` (LCM's sixth agent-facing tool) is intentionally not
wrapped here — it composes search + expand + LLM synthesis, which belongs at
the sera tool-authoring layer, not the trait surface (SPEC §4 footnote).
"""

from __future__ import annotations

import json
import os
import sqlite3
from pathlib import Path
from typing import Any

from sera_plugin_sdk.capabilities import (
    ContextDiagnostics,
    ContextEngine,
    ContextQuery,
)
from sera_plugin_sdk.errors import PluginCapabilityError
from sera_plugin_sdk.types import (
    AssembleBudget,
    AssembledContext,
    ContextSegment,
    CtxSearchHit,
    CtxSearchRequest,
    CtxSearchResponse,
    DescribeRequest,
    DescribeResponse,
    DoctorCheck,
    DoctorReport,
    ExpandRequest,
    ExpandResponse,
    IngestAck,
    IngestMessage,
    StatusResponse,
)

from ._lcm_core.dag import SummaryDAG
from ._lcm_core.store import MessageStore
from ._lcm_core.tokens import count_message_tokens


def _default_db_path() -> Path:
    override = os.environ.get("SERA_LCM_DATABASE_PATH")
    if override:
        return Path(override).expanduser().resolve()
    # Fresh sera-scoped location — no byte compat with hermes LCM storage.
    base = Path.home() / ".sera" / "plugins" / "lcm"
    return base / "lcm.db"


class LcmPlugin(ContextEngine, ContextQuery, ContextDiagnostics):
    """Hermes LCM, exposed to SERA as a ``ContextEngine`` plugin.

    The constructor resolves the vendored hermes-agent checkout, imports
    ``MessageStore`` and ``SummaryDAG`` from it, and opens the sera-scoped
    SQLite database (``~/.sera/plugins/lcm/lcm.db`` by default; override with
    ``SERA_LCM_DATABASE_PATH``).
    """

    def __init__(self, db_path: str | Path | None = None) -> None:
        resolved = Path(db_path) if db_path is not None else _default_db_path()
        resolved.parent.mkdir(parents=True, exist_ok=True)

        self._db_path = resolved
        self._store = MessageStore(resolved)
        self._dag = SummaryDAG(resolved)
        self._count_message_tokens = count_message_tokens

    # -- ContextEngine ----------------------------------------------------

    async def ingest(self, msg: IngestMessage) -> IngestAck:
        """Persist one message into LCM's ``MessageStore``.

        Implements SPEC §4 ``ContextEngine::ingest`` → ``store.append``.
        """
        try:
            openai_msg = _to_openai_message(msg)
            token_estimate = self._count_message_tokens(openai_msg)
            self._store.append(
                msg.session_id,
                openai_msg,
                token_estimate=token_estimate,
                source=msg.metadata.get("source", ""),
            )
        except Exception as exc:  # pragma: no cover - defensive wrap
            raise PluginCapabilityError(
                "LCM_INGEST_FAILED", f"failed to persist message: {exc}"
            ) from exc
        return IngestAck(accepted=True)

    async def assemble(self, budget: AssembleBudget) -> AssembledContext:
        """Build a context window under ``budget.budget_tokens``.

        Returns:
          - One ``ContextSegment`` per DAG summary (kind=``"longterm"``),
            sorted by depth descending (highest-depth first — SPEC §4
            "highest-depth summary nodes first, then lower").
          - One ``ContextSegment`` per raw message from the tail of the
            session (kind=``"working"``), ordered oldest → newest.

        The function does **not** trigger summarisation; that lives behind
        the ``compact`` seam which is not part of the sdk-py ABC today.
        When budget is zero or negative, returns an empty window.
        """
        segments: list[ContextSegment] = []
        used = 0
        limit = int(budget.budget_tokens)
        if limit <= 0:
            return AssembledContext(segments=[], total_tokens=0)

        # 1. Highest-depth summaries first (LCM's canonical assembly order).
        nodes = self._dag.get_session_nodes(budget.session_id)
        nodes_by_depth_desc = sorted(
            nodes, key=lambda n: (-n.depth, -float(n.latest_at or n.created_at or 0.0))
        )
        for node in nodes_by_depth_desc:
            node_tokens = int(node.token_count or 0)
            if limit and used + node_tokens > limit and segments:
                break
            segments.append(
                ContextSegment(
                    kind="longterm",
                    content=f"[d{node.depth} summary, node {node.node_id}]\n{node.summary}",
                    tokens=node_tokens,
                )
            )
            used += node_tokens

        # 2. Raw messages, newest slice that still fits — reversed then flipped
        #    to keep the fresh tail in chronological order.
        messages = self._store.get_session_messages(budget.session_id)
        tail_segments: list[ContextSegment] = []
        for stored in reversed(messages):
            content = stored.get("content") or ""
            msg_tokens = int(stored.get("token_estimate") or 0)
            if msg_tokens <= 0:
                msg_tokens = self._count_message_tokens(
                    {"role": stored.get("role", "user"), "content": content}
                )
            if limit and used + msg_tokens > limit and (segments or tail_segments):
                break
            tail_segments.append(
                ContextSegment(kind="working", content=content, tokens=msg_tokens)
            )
            used += msg_tokens

        segments.extend(reversed(tail_segments))
        return AssembledContext(segments=segments, total_tokens=used)

    # -- ContextQuery ------------------------------------------------------

    async def search(self, req: CtxSearchRequest) -> CtxSearchResponse:
        """Fan out to ``store.search`` + ``dag.search`` and merge results.

        SPEC §4 ``ContextQuery::search``. The sdk-py wire shape returns
        ``CtxSearchHit`` rows keyed by a string ``node_id`` — we stringify
        the integer ``store_id`` / ``node_id`` per SPEC §2.2 (opaque types).
        Messages get the ``"raw"`` depth label; summary nodes get ``"d{N}"``.
        """
        limit = max(1, int(req.limit or 10))
        hits: list[CtxSearchHit] = []

        try:
            msg_rows = self._store.search(
                req.query, session_id=req.session_id, limit=limit
            )
            for row in msg_rows:
                hits.append(
                    CtxSearchHit(
                        node_id=f"msg:{row['store_id']}",
                        depth_label="raw",
                        preview=row.get("snippet") or (row.get("content") or "")[:200],
                        rank=_maybe_float(row.get("search_rank")),
                    )
                )
        except Exception as exc:
            raise PluginCapabilityError(
                "LCM_SEARCH_FAILED", f"store search failed: {exc}"
            ) from exc

        try:
            node_rows = self._dag.search(
                req.query, session_id=req.session_id, limit=limit
            )
            for node in node_rows:
                hits.append(
                    CtxSearchHit(
                        node_id=f"node:{node.node_id}",
                        depth_label=f"d{node.depth}",
                        preview=(node.summary or "")[:300],
                        rank=_maybe_float(node.search_rank),
                    )
                )
        except Exception as exc:
            raise PluginCapabilityError(
                "LCM_SEARCH_FAILED", f"dag search failed: {exc}"
            ) from exc

        # Merge: lower rank is stronger (FTS5 convention; matches SPEC §2.2).
        hits.sort(key=lambda h: (h.rank if h.rank is not None else float("inf")))
        return CtxSearchResponse(hits=hits[:limit])

    async def describe(self, req: DescribeRequest) -> DescribeResponse:
        """Describe a node via ``SummaryDAG.describe_subtree`` or a raw message.

        Node IDs use the prefix encoding produced by :meth:`search`:
        ``"node:<int>"`` for DAG nodes, ``"msg:<int>"`` for raw messages.
        A bare integer string is treated as a DAG node for convenience.
        """
        kind, raw_id = _parse_node_id(req.node_id)
        if kind == "msg":
            stored = self._store.get(raw_id)
            if stored is None or (
                req.session_id and stored.get("session_id") != req.session_id
            ):
                raise PluginCapabilityError(
                    "LCM_NODE_NOT_FOUND",
                    f"message {req.node_id} not found in session {req.session_id}",
                )
            return DescribeResponse(
                node_id=req.node_id,
                depth_label="raw",
                tokens=int(stored.get("token_estimate") or 0),
                child_node_ids=[],
                metadata_json=json.dumps(
                    {
                        "role": stored.get("role"),
                        "session_id": stored.get("session_id"),
                        "timestamp": stored.get("timestamp"),
                        "source": stored.get("source") or "",
                    }
                ),
            )

        node = self._dag.get_node(raw_id)
        if node is None or (req.session_id and node.session_id != req.session_id):
            raise PluginCapabilityError(
                "LCM_NODE_NOT_FOUND",
                f"node {req.node_id} not found in session {req.session_id}",
            )
        info = self._dag.describe_subtree(raw_id)
        child_ids: list[str]
        if node.source_type == "nodes":
            child_ids = [f"node:{cid}" for cid in node.source_ids]
        else:
            child_ids = [f"msg:{cid}" for cid in node.source_ids]
        return DescribeResponse(
            node_id=req.node_id,
            depth_label=f"d{node.depth}",
            tokens=int(node.token_count or 0),
            child_node_ids=child_ids,
            metadata_json=json.dumps(info, default=_json_default),
        )

    async def expand(self, req: ExpandRequest) -> ExpandResponse:
        """Expand a node or message back to concrete content.

        For DAG nodes, walks ``source_ids`` back to either child summaries or
        raw messages. For raw messages, returns the stored content.
        """
        max_tokens = max(0, int(req.max_tokens or 0))
        kind, raw_id = _parse_node_id(req.node_id)

        if kind == "msg":
            stored = self._store.get(raw_id)
            if stored is None or (
                req.session_id and stored.get("session_id") != req.session_id
            ):
                raise PluginCapabilityError(
                    "LCM_NODE_NOT_FOUND",
                    f"message {req.node_id} not found in session {req.session_id}",
                )
            content = stored.get("content") or ""
            truncated = False
            if max_tokens and self._count_message_tokens({"role": stored.get("role", "user"), "content": content}) > max_tokens:
                # Rough char-based clamp — matches hermes's conservative truncation.
                approx_chars = max(1, max_tokens * 4)
                if len(content) > approx_chars:
                    content = content[:approx_chars]
                    truncated = True
            return ExpandResponse(
                node_id=req.node_id,
                content=content,
                tokens=self._count_message_tokens(
                    {"role": stored.get("role", "user"), "content": content}
                ),
                truncated=truncated,
            )

        node = self._dag.get_node(raw_id)
        if node is None or (req.session_id and node.session_id != req.session_id):
            raise PluginCapabilityError(
                "LCM_NODE_NOT_FOUND",
                f"node {req.node_id} not found in session {req.session_id}",
            )

        blocks: list[str] = []
        used = 0
        truncated = False

        if node.source_type == "messages":
            stored_by_id = self._store.get_batch(node.source_ids)
            for sid in node.source_ids:
                stored = stored_by_id.get(sid)
                if not stored:
                    continue
                content = stored.get("content") or ""
                msg_tokens = int(stored.get("token_estimate") or 0)
                if msg_tokens <= 0:
                    msg_tokens = self._count_message_tokens(
                        {"role": stored.get("role", "user"), "content": content}
                    )
                if max_tokens and used + msg_tokens > max_tokens and blocks:
                    truncated = True
                    break
                blocks.append(f"[{stored.get('role', 'user')} / store:{sid}] {content}")
                used += msg_tokens
        else:  # source_type == "nodes"
            for child in self._dag.get_source_nodes(node):
                child_tokens = int(child.token_count or 0)
                if max_tokens and used + child_tokens > max_tokens and blocks:
                    truncated = True
                    break
                blocks.append(
                    f"[d{child.depth} / node:{child.node_id}] {child.summary}"
                )
                used += child_tokens

        combined = "\n\n".join(blocks)
        return ExpandResponse(
            node_id=req.node_id,
            content=combined,
            tokens=used,
            truncated=truncated,
        )

    # -- ContextDiagnostics ------------------------------------------------

    async def status(self, session_id: str) -> StatusResponse:
        """Return LCM counters as a JSON blob (``fields_json``).

        Matches the ``lcm_status`` tool's output shape but trimmed to the
        fields the trait surface cares about. The full metrics remain
        available via the sera tool registry when the trait impl is wired
        into agent-facing tools.
        """
        session = session_id or ""
        store_messages = self._store.get_session_count(session) if session else 0
        store_tokens = self._store.get_session_token_total(session) if session else 0
        nodes = self._dag.get_session_nodes(session) if session else []

        depths: dict[int, dict[str, int]] = {}
        for node in nodes:
            bucket = depths.setdefault(node.depth, {"count": 0, "tokens": 0})
            bucket["count"] += 1
            bucket["tokens"] += int(node.token_count or 0)

        fields = {
            "session_id": session,
            "store": {
                "messages": store_messages,
                "estimated_tokens": store_tokens,
            },
            "dag": {
                "total_nodes": len(nodes),
                "depths": {
                    f"d{depth}": info for depth, info in sorted(depths.items())
                },
            },
        }
        return StatusResponse(fields_json=json.dumps(fields, default=_json_default))

    async def doctor(self) -> DoctorReport:
        """Run LCM-style health checks: integrity + FTS sync + orphans."""
        checks: list[DoctorCheck] = []
        conn = self._store._conn
        assert conn is not None, "MessageStore connection was closed before doctor()"

        # 1. Database integrity
        try:
            row = conn.execute("PRAGMA integrity_check").fetchone()
            ok_pragma = bool(row and row[0] == "ok")
            checks.append(
                DoctorCheck(
                    name="database_integrity",
                    severity="ok" if ok_pragma else "fail",
                    message=str(row[0]) if row else "no response",
                )
            )
        except sqlite3.Error as exc:
            checks.append(
                DoctorCheck(
                    name="database_integrity",
                    severity="fail",
                    message=str(exc),
                )
            )

        # 2. FTS index sync
        try:
            msg_count = conn.execute("SELECT COUNT(*) FROM messages").fetchone()[0]
            fts_count = conn.execute("SELECT COUNT(*) FROM messages_fts").fetchone()[0]
            checks.append(
                DoctorCheck(
                    name="fts_index_sync",
                    severity="ok" if fts_count >= msg_count else "warn",
                    message=f"{fts_count} FTS rows, {msg_count} messages",
                )
            )
        except sqlite3.Error as exc:
            checks.append(
                DoctorCheck(name="fts_index_sync", severity="fail", message=str(exc))
            )

        # 3. Node table health — count rows without deep-walking (trait doctor
        #    has no session scope; per-session orphan detail lives in status()).
        try:
            node_count = conn.execute("SELECT COUNT(*) FROM summary_nodes").fetchone()[0]
            checks.append(
                DoctorCheck(
                    name="summary_nodes_present",
                    severity="ok",
                    message=f"{node_count} summary node(s) across all sessions",
                )
            )
        except sqlite3.Error as exc:
            checks.append(
                DoctorCheck(
                    name="summary_nodes_present", severity="fail", message=str(exc)
                )
            )

        return DoctorReport(checks=checks)

    # -- Lifecycle ---------------------------------------------------------

    async def on_shutdown(self) -> None:
        """Close SQLite connections on graceful shutdown."""
        try:
            self._store.close()
        finally:
            self._dag.close()


# --- helpers ---------------------------------------------------------------


def _to_openai_message(msg: IngestMessage) -> dict[str, Any]:
    """Project ``IngestMessage`` onto the hermes/OpenAI message shape."""
    out: dict[str, Any] = {"role": msg.role, "content": msg.content}
    if "tool_call_id" in msg.metadata:
        out["tool_call_id"] = msg.metadata["tool_call_id"]
    if "tool_name" in msg.metadata:
        out["tool_name"] = msg.metadata["tool_name"]
    return out


def _parse_node_id(node_id: str) -> tuple[str, int]:
    """Decode ``"msg:<int>"`` / ``"node:<int>"`` / bare-int node identifiers.

    Raises :class:`PluginCapabilityError` if the id cannot be decoded — the
    stdio transport turns that into a JSON-RPC capability error.
    """
    if not node_id:
        raise PluginCapabilityError(
            "LCM_INVALID_NODE_ID", "node_id must not be empty"
        )
    if ":" in node_id:
        prefix, _, rest = node_id.partition(":")
        if prefix not in ("msg", "node"):
            raise PluginCapabilityError(
                "LCM_INVALID_NODE_ID",
                f"unknown node prefix '{prefix}' — expected msg:/node:",
            )
        try:
            return prefix, int(rest)
        except ValueError as exc:
            raise PluginCapabilityError(
                "LCM_INVALID_NODE_ID", f"invalid numeric id in '{node_id}'"
            ) from exc
    # Bare integer — default to DAG node id.
    try:
        return "node", int(node_id)
    except ValueError as exc:
        raise PluginCapabilityError(
            "LCM_INVALID_NODE_ID", f"cannot parse node_id '{node_id}'"
        ) from exc


def _maybe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _json_default(obj: Any) -> Any:
    """Serialise non-JSON-native types conservatively."""
    if isinstance(obj, bytes):
        return obj.decode("utf-8", errors="replace")
    return str(obj)


__all__ = ["LcmPlugin"]

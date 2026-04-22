"""ContextEngine trait: ``ingest`` + ``assemble``."""

from __future__ import annotations

import pytest
from sera_plugin_sdk.capabilities import (
    ContextDiagnostics,
    ContextEngine,
    ContextQuery,
)
from sera_plugin_sdk.types import (
    AssembleBudget,
    IngestMessage,
)

from sera_context_lcm import LcmPlugin


async def test_plugin_implements_all_three_capabilities(plugin: LcmPlugin) -> None:
    assert isinstance(plugin, ContextEngine)
    assert isinstance(plugin, ContextQuery)
    assert isinstance(plugin, ContextDiagnostics)


async def test_ingest_persists_message(plugin: LcmPlugin) -> None:
    ack = await plugin.ingest(
        IngestMessage(session_id="s1", role="user", content="hello")
    )
    assert ack.accepted is True
    assert plugin._store.get_session_count("s1") == 1


async def test_ingest_round_trip_preserves_role_and_content(plugin: LcmPlugin) -> None:
    await plugin.ingest(
        IngestMessage(session_id="s1", role="assistant", content="reply one")
    )
    messages = plugin._store.get_session_messages("s1")
    assert len(messages) == 1
    assert messages[0]["role"] == "assistant"
    assert messages[0]["content"] == "reply one"


async def test_ingest_stores_source_from_metadata(plugin: LcmPlugin) -> None:
    await plugin.ingest(
        IngestMessage(
            session_id="s2",
            role="user",
            content="tagged",
            metadata={"source": "telegram"},
        )
    )
    messages = plugin._store.get_session_messages("s2")
    assert messages[0]["source"] == "telegram"


async def test_assemble_returns_working_segments_in_order(plugin: LcmPlugin) -> None:
    for i in range(3):
        await plugin.ingest(
            IngestMessage(session_id="s1", role="user", content=f"msg-{i}")
        )
    assembled = await plugin.assemble(AssembleBudget(session_id="s1", budget_tokens=200))
    contents = [s.content for s in assembled.segments]
    assert contents == ["msg-0", "msg-1", "msg-2"]
    assert all(s.kind == "working" for s in assembled.segments)
    assert assembled.total_tokens > 0


async def test_assemble_respects_budget(plugin: LcmPlugin) -> None:
    # Each ingest adds one small message (~5 tokens via char heuristic).
    for i in range(10):
        await plugin.ingest(
            IngestMessage(session_id="s1", role="user", content=f"row-{i}")
        )
    tight = await plugin.assemble(AssembleBudget(session_id="s1", budget_tokens=20))
    assert tight.total_tokens <= 40  # allow slack for per-message overhead
    assert len(tight.segments) < 10


async def test_assemble_empty_session_returns_empty_window(plugin: LcmPlugin) -> None:
    assembled = await plugin.assemble(AssembleBudget(session_id="never", budget_tokens=1000))
    assert assembled.segments == []
    assert assembled.total_tokens == 0


async def test_assemble_keeps_freshest_tail_under_tight_budget(plugin: LcmPlugin) -> None:
    # Each message distinct so ordering is visible. Older messages should fall
    # out first; the newest content must survive.
    for i in range(6):
        await plugin.ingest(
            IngestMessage(session_id="s1", role="user", content=f"item-{i}")
        )
    assembled = await plugin.assemble(AssembleBudget(session_id="s1", budget_tokens=20))
    assert assembled.segments[-1].content == "item-5"


async def test_assemble_places_longterm_segments_before_working(plugin: LcmPlugin) -> None:
    # Inject a synthetic DAG node so we exercise the longterm branch without
    # needing the full summarisation pipeline.
    from sera_context_lcm._lcm_core.dag import SummaryNode

    await plugin.ingest(
        IngestMessage(session_id="s1", role="user", content="leaf message")
    )
    node = SummaryNode(
        session_id="s1",
        depth=1,
        summary="synthetic summary of older turns",
        token_count=6,
        source_token_count=50,
        source_ids=[1],
        source_type="messages",
    )
    plugin._dag.add_node(node)

    assembled = await plugin.assemble(AssembleBudget(session_id="s1", budget_tokens=500))
    kinds = [s.kind for s in assembled.segments]
    # longterm must come first.
    assert kinds[0] == "longterm"
    assert "working" in kinds


@pytest.mark.parametrize("budget", [0, -1])
async def test_assemble_zero_or_negative_budget_is_empty(
    plugin: LcmPlugin, budget: int
) -> None:
    await plugin.ingest(
        IngestMessage(session_id="s1", role="user", content="x")
    )
    assembled = await plugin.assemble(AssembleBudget(session_id="s1", budget_tokens=budget))
    assert assembled.segments == []
    assert assembled.total_tokens == 0

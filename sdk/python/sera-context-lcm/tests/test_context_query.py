"""ContextQuery trait: ``search`` + ``describe`` + ``expand``."""

from __future__ import annotations

import json

import pytest
from sera_plugin_sdk.errors import PluginCapabilityError
from sera_plugin_sdk.types import (
    CtxSearchRequest,
    DescribeRequest,
    ExpandRequest,
    IngestMessage,
)

from sera_context_lcm import LcmPlugin
from sera_context_lcm._lcm_core.dag import SummaryNode


async def _seed(plugin: LcmPlugin, session_id: str = "s1") -> None:
    await plugin.ingest(
        IngestMessage(session_id=session_id, role="user", content="hello world sera")
    )
    await plugin.ingest(
        IngestMessage(
            session_id=session_id, role="assistant", content="the sera runtime is online"
        )
    )
    await plugin.ingest(
        IngestMessage(session_id=session_id, role="user", content="wat is the plan")
    )


async def test_search_returns_matching_messages(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    resp = await plugin.search(
        CtxSearchRequest(session_id="s1", query="sera", limit=10)
    )
    assert len(resp.hits) >= 2
    for hit in resp.hits:
        assert hit.node_id.startswith("msg:")
        assert hit.depth_label == "raw"
        assert "sera" in hit.preview.lower()


async def test_search_mixes_message_and_summary_hits(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    node = SummaryNode(
        session_id="s1",
        depth=1,
        summary="sera runtime summary of the conversation",
        token_count=7,
        source_token_count=30,
        source_ids=[1, 2],
        source_type="messages",
    )
    plugin._dag.add_node(node)

    resp = await plugin.search(
        CtxSearchRequest(session_id="s1", query="sera", limit=10)
    )
    node_ids = {h.node_id for h in resp.hits}
    assert any(nid.startswith("node:") for nid in node_ids)
    assert any(nid.startswith("msg:") for nid in node_ids)


async def test_search_honours_limit(plugin: LcmPlugin) -> None:
    for i in range(6):
        await plugin.ingest(
            IngestMessage(
                session_id="s1", role="user", content=f"sera sera sera #{i}"
            )
        )
    resp = await plugin.search(
        CtxSearchRequest(session_id="s1", query="sera", limit=3)
    )
    assert len(resp.hits) <= 3


async def test_describe_raw_message(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    resp = await plugin.describe(DescribeRequest(session_id="s1", node_id="msg:1"))
    assert resp.node_id == "msg:1"
    assert resp.depth_label == "raw"
    assert resp.child_node_ids == []
    meta = json.loads(resp.metadata_json)
    assert meta["role"] == "user"
    assert meta["session_id"] == "s1"


async def test_describe_dag_node(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    node = SummaryNode(
        session_id="s1",
        depth=1,
        summary="abc",
        token_count=5,
        source_token_count=30,
        source_ids=[1, 2],
        source_type="messages",
    )
    node_id = plugin._dag.add_node(node)

    resp = await plugin.describe(
        DescribeRequest(session_id="s1", node_id=f"node:{node_id}")
    )
    assert resp.depth_label == "d1"
    assert resp.tokens == 5
    assert resp.child_node_ids == ["msg:1", "msg:2"]


async def test_describe_bare_int_defaults_to_dag_node(plugin: LcmPlugin) -> None:
    node_id = plugin._dag.add_node(
        SummaryNode(
            session_id="s1",
            depth=2,
            summary="x",
            token_count=1,
            source_token_count=1,
            source_ids=[],
            source_type="nodes",
        )
    )
    resp = await plugin.describe(
        DescribeRequest(session_id="s1", node_id=str(node_id))
    )
    assert resp.depth_label == "d2"


async def test_describe_missing_message_errors(plugin: LcmPlugin) -> None:
    with pytest.raises(PluginCapabilityError) as excinfo:
        await plugin.describe(DescribeRequest(session_id="s1", node_id="msg:999"))
    assert excinfo.value.code == "LCM_NODE_NOT_FOUND"


async def test_describe_wrong_session_errors(plugin: LcmPlugin) -> None:
    await _seed(plugin, session_id="sA")
    with pytest.raises(PluginCapabilityError):
        await plugin.describe(DescribeRequest(session_id="sB", node_id="msg:1"))


@pytest.mark.parametrize("bad", ["", "msg:", "xyz:1", "msg:abc"])
async def test_describe_invalid_id_errors(plugin: LcmPlugin, bad: str) -> None:
    with pytest.raises(PluginCapabilityError) as excinfo:
        await plugin.describe(DescribeRequest(session_id="s1", node_id=bad))
    assert excinfo.value.code == "LCM_INVALID_NODE_ID"


async def test_expand_raw_message_returns_content(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    resp = await plugin.expand(
        ExpandRequest(session_id="s1", node_id="msg:1", max_tokens=200)
    )
    assert resp.content == "hello world sera"
    assert resp.tokens > 0
    assert resp.truncated is False


async def test_expand_message_truncates_when_over_budget(plugin: LcmPlugin) -> None:
    await plugin.ingest(
        IngestMessage(session_id="s1", role="user", content="a" * 10_000)
    )
    resp = await plugin.expand(
        ExpandRequest(session_id="s1", node_id="msg:1", max_tokens=20)
    )
    assert resp.truncated is True
    assert len(resp.content) < 10_000


async def test_expand_dag_node_with_message_sources(plugin: LcmPlugin) -> None:
    await _seed(plugin)
    node_id = plugin._dag.add_node(
        SummaryNode(
            session_id="s1",
            depth=0,
            summary="leaf summary",
            token_count=4,
            source_token_count=20,
            source_ids=[1, 2],
            source_type="messages",
        )
    )
    resp = await plugin.expand(
        ExpandRequest(session_id="s1", node_id=f"node:{node_id}", max_tokens=500)
    )
    assert "hello world sera" in resp.content
    assert "the sera runtime is online" in resp.content
    assert resp.tokens > 0


async def test_expand_dag_node_with_node_sources(plugin: LcmPlugin) -> None:
    leaf_id = plugin._dag.add_node(
        SummaryNode(
            session_id="s1",
            depth=0,
            summary="leaf-0 summary",
            token_count=6,
            source_token_count=20,
            source_ids=[],
            source_type="messages",
        )
    )
    parent_id = plugin._dag.add_node(
        SummaryNode(
            session_id="s1",
            depth=1,
            summary="parent",
            token_count=3,
            source_token_count=6,
            source_ids=[leaf_id],
            source_type="nodes",
        )
    )
    resp = await plugin.expand(
        ExpandRequest(session_id="s1", node_id=f"node:{parent_id}", max_tokens=500)
    )
    assert "leaf-0 summary" in resp.content
    assert resp.tokens > 0

"""ContextDiagnostics trait: ``status`` + ``doctor``."""

from __future__ import annotations

import json

from sera_plugin_sdk.types import IngestMessage

from sera_context_lcm import LcmPlugin
from sera_context_lcm._lcm_core.dag import SummaryNode


async def test_status_empty_session_reports_zero_counts(plugin: LcmPlugin) -> None:
    resp = await plugin.status("empty-session")
    data = json.loads(resp.fields_json)
    assert data["session_id"] == "empty-session"
    assert data["store"]["messages"] == 0
    assert data["dag"]["total_nodes"] == 0


async def test_status_reports_message_counts(plugin: LcmPlugin) -> None:
    for i in range(4):
        await plugin.ingest(
            IngestMessage(session_id="s1", role="user", content=f"msg-{i}")
        )
    resp = await plugin.status("s1")
    data = json.loads(resp.fields_json)
    assert data["store"]["messages"] == 4
    assert data["store"]["estimated_tokens"] > 0


async def test_status_groups_dag_nodes_by_depth(plugin: LcmPlugin) -> None:
    for depth in (0, 0, 1):
        plugin._dag.add_node(
            SummaryNode(
                session_id="s1",
                depth=depth,
                summary="x",
                token_count=3,
                source_token_count=10,
                source_ids=[],
                source_type="messages",
            )
        )
    resp = await plugin.status("s1")
    data = json.loads(resp.fields_json)
    assert data["dag"]["total_nodes"] == 3
    assert data["dag"]["depths"]["d0"]["count"] == 2
    assert data["dag"]["depths"]["d1"]["count"] == 1


async def test_doctor_reports_core_checks(plugin: LcmPlugin) -> None:
    report = await plugin.doctor()
    names = {c.name for c in report.checks}
    assert {"database_integrity", "fts_index_sync", "summary_nodes_present"} <= names
    # Freshly-booted DB should be healthy.
    integrity = next(c for c in report.checks if c.name == "database_integrity")
    assert integrity.severity == "ok"


async def test_doctor_fts_in_sync_after_ingest(plugin: LcmPlugin) -> None:
    for i in range(3):
        await plugin.ingest(
            IngestMessage(session_id="s1", role="user", content=f"m-{i}")
        )
    report = await plugin.doctor()
    fts = next(c for c in report.checks if c.name == "fts_index_sync")
    assert fts.severity == "ok"
    assert "3" in fts.message

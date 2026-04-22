"""Shared pytest fixtures."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest

from sera_context_lcm import LcmPlugin


@pytest.fixture
def plugin(tmp_path: Path) -> Iterator[LcmPlugin]:
    """Return an ``LcmPlugin`` backed by a per-test SQLite file."""
    db_path = tmp_path / "lcm.db"
    p = LcmPlugin(db_path=db_path)
    try:
        yield p
    finally:
        p._store.close()
        p._dag.close()

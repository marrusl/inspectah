"""Tests for pipeline.py: load_snapshot / save_snapshot."""

import json
import tempfile
from pathlib import Path

import pytest

from yoinkc.pipeline import load_snapshot, save_snapshot
from yoinkc.schema import InspectionSnapshot, SCHEMA_VERSION


def _write_snapshot(path: Path, data: dict) -> None:
    path.write_text(json.dumps(data))


def test_load_snapshot_version_mismatch_raises():
    """Loading a snapshot with a different schema version must raise ValueError."""
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "snap.json"
        _write_snapshot(p, {"schema_version": 999})
        with pytest.raises(ValueError, match="different version"):
            load_snapshot(p)


def test_load_snapshot_current_version_succeeds():
    """Loading a snapshot at the current schema version must succeed."""
    snapshot = InspectionSnapshot(meta={"host_root": "/host"})
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "snap.json"
        save_snapshot(snapshot, p)
        loaded = load_snapshot(p)
    assert loaded.schema_version == SCHEMA_VERSION

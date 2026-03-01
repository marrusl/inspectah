"""
Pipeline orchestrator: run inspectors (or load snapshot), redact, then run renderers.
All renderers write to output_dir (created if it does not exist).
"""

import json
import sys
from pathlib import Path
from typing import Callable, Optional

from .redact import redact_snapshot
from .schema import InspectionSnapshot, SCHEMA_VERSION


def load_snapshot(path: Path) -> InspectionSnapshot:
    """Load and deserialize an inspection snapshot from JSON."""
    data = json.loads(path.read_text())
    file_version = data.get("schema_version", 1)
    if file_version > SCHEMA_VERSION:
        print(
            f"WARNING: snapshot was created by a newer yoinkc (schema v{file_version}, "
            f"this tool supports v{SCHEMA_VERSION}). Some fields may be dropped.",
            file=sys.stderr,
        )
    return InspectionSnapshot.model_validate(data)


def save_snapshot(snapshot: InspectionSnapshot, path: Path) -> None:
    """Serialize snapshot to JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(snapshot.model_dump_json(indent=2))


def run_pipeline(
    *,
    host_root: Path,
    output_dir: Path,
    run_inspectors: Callable[[Path], InspectionSnapshot],
    run_renderers: Callable[[InspectionSnapshot, Path], None],
    from_snapshot_path: Optional[Path] = None,
    inspect_only: bool = False,
) -> InspectionSnapshot:
    """
    Either load snapshot from file or run inspectors; then optionally run renderers.

    Returns the snapshot (loaded or newly built).
    """
    if from_snapshot_path is not None:
        snapshot = load_snapshot(from_snapshot_path)
        snapshot = redact_snapshot(snapshot)
        if not inspect_only:
            output_dir.mkdir(parents=True, exist_ok=True)
            run_renderers(snapshot, output_dir)
        return snapshot

    snapshot = run_inspectors(host_root)
    snapshot = redact_snapshot(snapshot)
    output_dir.mkdir(parents=True, exist_ok=True)
    snapshot_path = output_dir / "inspection-snapshot.json"
    save_snapshot(snapshot, snapshot_path)

    if not inspect_only:
        run_renderers(snapshot, output_dir)

    return snapshot

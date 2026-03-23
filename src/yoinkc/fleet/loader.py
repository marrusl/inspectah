"""Discover and load inspection snapshots from a directory of tarballs/JSON files."""

import io
import json
import sys
import tarfile
import warnings
from pathlib import Path

from ..schema import InspectionSnapshot


def compute_display_names(hostnames: list[str]) -> list[str]:
    """Compute shortest unique display names from hostnames.

    Progressive disambiguation starts with the short hostname and adds one
    domain segment at a time until collisions are resolved. If identical
    hostnames still collide after all segments are exhausted, numeric suffixes
    are applied to the short hostname.
    """
    if not hostnames:
        return []

    segments = [hostname.split(".") if hostname else [""] for hostname in hostnames]
    depths = [1] * len(hostnames)

    def labels_for_depths() -> list[str]:
        return [
            ".".join(parts[: min(depth, len(parts))])
            for parts, depth in zip(segments, depths)
        ]

    while True:
        labels = labels_for_depths()
        groups: dict[str, list[int]] = {}
        for idx, label in enumerate(labels):
            groups.setdefault(label, []).append(idx)

        collisions = [indices for indices in groups.values() if len(indices) > 1]
        if not collisions:
            return labels

        changed = False
        for indices in collisions:
            for idx in indices:
                if depths[idx] < len(segments[idx]):
                    depths[idx] += 1
                    changed = True

        if not changed:
            break

    labels = labels_for_depths()
    duplicate_groups: dict[str, list[int]] = {}
    for idx, label in enumerate(labels):
        duplicate_groups.setdefault(label, []).append(idx)

    for indices in duplicate_groups.values():
        if len(indices) < 2:
            continue
        for ordinal, idx in enumerate(indices, start=1):
            labels[idx] = f"{segments[idx][0]} ({ordinal})"

    return labels


def assign_display_names(snapshots: list[InspectionSnapshot]) -> list[str]:
    """Compute and store per-snapshot display names in snapshot metadata."""
    hostnames = [
        snapshot.meta.get("hostname", f"host-{idx}")
        for idx, snapshot in enumerate(snapshots)
    ]
    display_names = compute_display_names(hostnames)
    for snapshot, display_name in zip(snapshots, display_names):
        snapshot.meta["display_name"] = display_name
    return display_names


def _load_from_json(path: Path) -> InspectionSnapshot | None:
    """Load a snapshot from a bare JSON file. Returns None on failure."""
    try:
        data = json.loads(path.read_text())
        return InspectionSnapshot(**data)
    except Exception as exc:
        warnings.warn(f"Skipping invalid JSON {path.name}: {exc}", stacklevel=2)
        return None


def _load_from_tarball(path: Path) -> InspectionSnapshot | None:
    """Extract inspection-snapshot.json from a tarball. Returns None on failure."""
    try:
        with tarfile.open(path, "r:gz") as tar:
            for member in tar.getmembers():
                if member.name.endswith("inspection-snapshot.json"):
                    f = tar.extractfile(member)
                    if f is None:
                        continue
                    data = json.loads(f.read())
                    return InspectionSnapshot(**data)
        warnings.warn(
            f"Skipping tarball {path.name}: no inspection-snapshot.json found",
            stacklevel=2,
        )
        return None
    except Exception as exc:
        warnings.warn(f"Skipping tarball {path.name}: {exc}", stacklevel=2)
        return None


def discover_snapshots(input_dir: Path) -> list[InspectionSnapshot]:
    """Scan a directory for tarballs and JSON files, return loaded snapshots.

    Invalid files are skipped with a warning.
    """
    snapshots: list[InspectionSnapshot] = []
    for path in sorted(input_dir.iterdir()):
        if path.name == "fleet-snapshot.json":
            continue  # skip previous output to prevent self-contamination
        if path.suffix == ".gz" and path.name.endswith(".tar.gz"):
            snap = _load_from_tarball(path)
        elif path.suffix == ".json":
            snap = _load_from_json(path)
        else:
            continue
        if snap is not None:
            snapshots.append(snap)
    return snapshots


def validate_snapshots(snapshots: list[InspectionSnapshot]) -> None:
    """Validate that all snapshots are compatible for merging.

    Checks: minimum count, schema version, os_release, base_image.
    Exits with error message on failure.
    """
    if len(snapshots) < 2:
        print(f"Error: Need at least 2 snapshots, found {len(snapshots)}.", file=sys.stderr)
        sys.exit(1)

    # Schema version
    versions = {s.schema_version for s in snapshots}
    if len(versions) > 1:
        print(f"Error: Schema version mismatch: {versions}", file=sys.stderr)
        sys.exit(1)

    # Duplicate hostnames (warn, not error)
    hostnames = [s.meta.get("hostname", "") for s in snapshots]
    seen: set[str] = set()
    for h in hostnames:
        if h in seen:
            warnings.warn(f"Duplicate hostname: {h}", stacklevel=2)
        seen.add(h)

    # os_release — require present on all snapshots
    for s in snapshots:
        if not s.os_release:
            hostname = s.meta.get("hostname", "unknown")
            print(f"Error: Snapshot from {hostname} has no os_release.", file=sys.stderr)
            sys.exit(1)

    os_ids = {s.os_release.id for s in snapshots}
    if len(os_ids) > 1:
        print(f"Error: os_release.id mismatch: {os_ids}", file=sys.stderr)
        sys.exit(1)
    os_versions = {s.os_release.version_id for s in snapshots}
    if len(os_versions) > 1:
        print(f"Error: os_release.version_id mismatch: {os_versions}", file=sys.stderr)
        sys.exit(1)

    # base_image
    base_images: set[str] = set()
    for s in snapshots:
        if s.rpm and s.rpm.base_image:
            base_images.add(s.rpm.base_image)
    if len(base_images) > 1:
        print(f"Error: rpm.base_image mismatch: {base_images}", file=sys.stderr)
        sys.exit(1)

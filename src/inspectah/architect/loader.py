"""Load refined fleet tarballs for architect analysis."""

from __future__ import annotations

import json
import logging
import tarfile
from pathlib import Path

from inspectah.architect.analyzer import FleetInput

logger = logging.getLogger(__name__)


def load_refined_fleets(input_dir: Path) -> list[FleetInput]:
    """Load refined fleet tarballs from a directory.

    Each tarball should contain an inspection-snapshot.json with fleet metadata.
    Returns a list of FleetInput objects ready for the analyzer.
    """
    fleets: list[FleetInput] = []

    if not input_dir.exists():
        return fleets

    for path in sorted(input_dir.iterdir()):
        if not (path.suffix == ".gz" and path.name.endswith(".tar.gz")):
            continue

        try:
            snapshot = _extract_snapshot(path)
        except Exception as e:
            logger.warning("Skipping %s: %s", path.name, e)
            continue

        if snapshot is None:
            logger.warning("No inspection-snapshot.json found in %s", path.name)
            continue

        fleet_input = _snapshot_to_fleet_input(snapshot)
        fleets.append(fleet_input)

    return fleets


def _extract_snapshot(tarball_path: Path) -> dict | None:
    """Extract inspection-snapshot.json from a tarball."""
    with tarfile.open(tarball_path, "r:gz") as tar:
        for member in tar.getmembers():
            if member.name.endswith("inspection-snapshot.json"):
                f = tar.extractfile(member)
                if f is None:
                    continue
                return json.loads(f.read())
    return None


def _snapshot_to_fleet_input(snapshot: dict) -> FleetInput:
    """Convert an inspection snapshot dict to a FleetInput."""
    meta = snapshot.get("meta", {})
    hostname = meta.get("hostname", "unknown")
    fleet_meta = meta.get("fleet", {})
    host_count = fleet_meta.get("total_hosts", 1)

    # Extract package NVRAs
    rpm = snapshot.get("rpm", {})
    packages = [
        pkg.get("nvra", pkg.get("name", ""))
        for pkg in rpm.get("packages_added", [])
        if pkg.get("nvra") or pkg.get("name")
    ]

    # Extract config file paths
    config = snapshot.get("config", {})
    configs = [f.get("path", "") for f in config.get("files", []) if f.get("path")]

    base_image = rpm.get("base_image", "")

    # Extract preflight data
    preflight = snapshot.get("preflight", {})
    unavailable_packages = list(preflight.get("unavailable", [])) if preflight else []
    direct_install_packages = list(preflight.get("direct_install", [])) if preflight else []
    unverifiable = preflight.get("unverifiable", []) if preflight else []
    unverifiable_packages = [uv.get("name", "") for uv in unverifiable if uv.get("name")]
    preflight_status = preflight.get("status", "skipped") if preflight else "skipped"

    return FleetInput(
        name=hostname,
        packages=packages,
        configs=configs,
        host_count=host_count,
        base_image=base_image if isinstance(base_image, str) else "",
        unavailable_packages=unavailable_packages,
        direct_install_packages=direct_install_packages,
        unverifiable_packages=unverifiable_packages,
        preflight_status=preflight_status,
    )

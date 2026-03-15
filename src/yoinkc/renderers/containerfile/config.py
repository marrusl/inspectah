"""Containerfile section: configuration files (consolidated COPY)."""

from pathlib import Path

from ...schema import InspectionSnapshot
from ._config_tree import config_copy_roots, config_inventory_comment


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    output_dir: Path,
    dhcp_paths: set,
) -> list[str]:
    """Return Containerfile lines for config COPY and CA trust store."""
    lines: list[str] = []

    # All captured config files, repo files, firewall, timers, NM connections,
    # kernel module configs, sysctl overrides, audit rules, and tmpfiles.d are
    # written under config/ and copied to the image root in a single layer.
    lines.append("# === Configuration Files ===")
    inventory_lines = config_inventory_comment(snapshot, dhcp_paths)
    lines.extend(inventory_lines)
    if any(f.diff_against_rpm for f in (snapshot.config.files if snapshot.config else [])):
        lines.append("# Config diffs (--config-diffs): see audit-report.md and report.html for per-file diffs.")
    lines.append("")

    # Emit one COPY per non-empty top-level dir under config/ (excluding tmp/).
    config_dir = output_dir / "config"
    roots = config_copy_roots(config_dir)
    for root in roots:
        lines.append(f"COPY config/{root}/ /{root}/")
    if not roots:
        lines.append("# (no config files captured)")
    lines.append("")

    # CA trust anchors — run update-ca-trust if custom certs were captured
    _CA_ANCHOR_PREFIX = "etc/pki/ca-trust/source/anchors/"
    has_ca_anchors = snapshot.config and any(
        f.include and f.path.lstrip("/").startswith(_CA_ANCHOR_PREFIX)
        for f in snapshot.config.files
    )
    if has_ca_anchors:
        lines.append("# === CA Trust Store ===")
        lines.append("# Custom CA certificates detected in /etc/pki/ca-trust/source/anchors/")
        lines.append("RUN update-ca-trust")
        lines.append("")

    return lines

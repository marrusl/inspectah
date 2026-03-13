"""Triage computation shared by audit report and HTML renderers."""

import warnings
from pathlib import Path

from ..schema import InspectionSnapshot

_QUADLET_PREFIX = "etc/containers/systemd/"


def _count_containerfile_fixmes(output_dir: Path) -> int:
    """Count ``# FIXME`` comment lines in the generated Containerfile."""
    cf = output_dir / "Containerfile"
    if not cf.exists():
        return 0
    try:
        return sum(
            1
            for line in cf.read_text().splitlines()
            if line.strip().startswith("#") and "FIXME" in line
        )
    except OSError as exc:
        warnings.warn(f"Could not read {cf} for FIXME count: {exc}", stacklevel=2)
        return 0


def _config_file_count(snapshot: InspectionSnapshot) -> int:
    return sum(
        1
        for f in (snapshot.config.files if snapshot.config else [])
        if f.include and not f.path.lstrip("/").startswith(_QUADLET_PREFIX)
    )


def compute_triage(snapshot: InspectionSnapshot, output_dir: Path) -> dict:
    """Classify inspected items into automatic / fixme / manual buckets.

    Returns {"automatic": int, "fixme": int, "manual": int}.
    Must be called after the Containerfile has been written.
    """
    automatic = 0
    if snapshot.rpm:
        automatic += sum(1 for p in snapshot.rpm.packages_added if p.include)
        automatic += sum(1 for p in snapshot.rpm.base_image_only if p.include)
    if snapshot.services:
        automatic += sum(
            1
            for sc in snapshot.services.state_changes
            if sc.include and sc.action in ("enable", "disable")
        )
    if snapshot.config:
        automatic += _config_file_count(snapshot)
    if snapshot.network and snapshot.network.firewall_zones:
        automatic += sum(1 for z in snapshot.network.firewall_zones if z.include)
    if snapshot.scheduled_tasks:
        automatic += sum(
            1 for t in (snapshot.scheduled_tasks.generated_timer_units or []) if t.include
        )
        automatic += sum(1 for t in snapshot.scheduled_tasks.systemd_timers if t.source == "local")
    if snapshot.users_groups:
        automatic += len(snapshot.users_groups.users or [])
        automatic += len(snapshot.users_groups.groups or [])
    if snapshot.containers and snapshot.containers.quadlet_units:
        automatic += sum(1 for q in snapshot.containers.quadlet_units if q.include)

    fixme = _count_containerfile_fixmes(output_dir)

    manual = len(snapshot.warnings or [])
    manual += len(snapshot.redactions or [])
    if snapshot.users_groups and snapshot.users_groups.ssh_authorized_keys_refs:
        manual += len(snapshot.users_groups.ssh_authorized_keys_refs)

    return {"automatic": automatic, "fixme": fixme, "manual": manual}


def compute_triage_detail(
    snapshot: InspectionSnapshot, output_dir: Path
) -> list:
    """Return per-item triage breakdown for the readiness panel.

    Each entry: {"label": str, "count": int, "tab": str,
                 "status": "automatic"|"fixme"|"manual"}.
    Only items with count > 0 are included.
    """
    items: list = []

    def _add(label: str, count: int, tab: str, status: str) -> None:
        if count > 0:
            items.append({"label": label, "count": count, "tab": tab, "status": status})

    # Automatic
    if snapshot.rpm:
        _add(
            "Packages added",
            sum(1 for p in snapshot.rpm.packages_added if p.include),
            "packages",
            "automatic",
        )
        _add(
            "New from base image",
            sum(1 for p in snapshot.rpm.base_image_only if p.include),
            "packages",
            "automatic",
        )
    if snapshot.services:
        n = sum(
            1
            for sc in snapshot.services.state_changes
            if sc.include and sc.action in ("enable", "disable")
        )
        _add("Services enabled/disabled", n, "services", "automatic")
    if snapshot.config:
        _add("Config files", _config_file_count(snapshot), "config", "automatic")
    if snapshot.containers and snapshot.containers.quadlet_units:
        _add(
            "Quadlet units",
            sum(1 for q in snapshot.containers.quadlet_units if q.include),
            "containers",
            "automatic",
        )
    if snapshot.users_groups:
        n = len(snapshot.users_groups.users or []) + len(snapshot.users_groups.groups or [])
        _add("Users/groups", n, "users_groups", "automatic")
    if snapshot.scheduled_tasks:
        n = sum(1 for t in (snapshot.scheduled_tasks.generated_timer_units or []) if t.include)
        n += sum(1 for t in snapshot.scheduled_tasks.systemd_timers if t.source == "local")
        _add("Cron-to-timer conversions", n, "scheduled_tasks", "automatic")
    if snapshot.network and snapshot.network.firewall_zones:
        _add(
            "Firewall zones",
            sum(1 for z in snapshot.network.firewall_zones if z.include),
            "network",
            "automatic",
        )

    # Needs review
    _add("Containerfile FIXMEs", _count_containerfile_fixmes(output_dir), "containerfile", "fixme")

    # Manual
    _add("Secrets redacted", len(snapshot.redactions or []), "secrets", "manual")
    _add("Warnings", len(snapshot.warnings or []), "warnings", "manual")
    if snapshot.users_groups and snapshot.users_groups.ssh_authorized_keys_refs:
        _add("SSH key references", len(snapshot.users_groups.ssh_authorized_keys_refs), "users_groups", "manual")

    return items

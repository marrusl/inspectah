"""Triage computation shared by audit report and HTML renderers."""

from pathlib import Path

from ..schema import InspectionSnapshot


def compute_triage(snapshot: InspectionSnapshot, output_dir: Path) -> dict:
    """Classify inspected items into automatic / fixme / manual buckets.

    Returns {"automatic": int, "fixme": int, "manual": int}.
    Must be called after the Containerfile has been written.
    """
    automatic = 0
    if snapshot.rpm:
        automatic += len(snapshot.rpm.packages_added)
    if snapshot.services:
        automatic += len(snapshot.services.enabled_units)
        automatic += len(snapshot.services.disabled_units)
    if snapshot.config:
        automatic += len(snapshot.config.files)
    if snapshot.network and snapshot.network.firewall_zones:
        automatic += len(snapshot.network.firewall_zones)
    if snapshot.scheduled_tasks:
        automatic += len(snapshot.scheduled_tasks.generated_timer_units or [])
        automatic += len([t for t in snapshot.scheduled_tasks.systemd_timers if t.source == "local"])
    if snapshot.users_groups:
        automatic += len(snapshot.users_groups.users or [])
        automatic += len(snapshot.users_groups.groups or [])
    if snapshot.containers and snapshot.containers.quadlet_units:
        automatic += len(snapshot.containers.quadlet_units)

    fixme = 0
    cf = output_dir / "Containerfile"
    if cf.exists():
        try:
            for line in cf.read_text().splitlines():
                if line.strip().startswith("#") and "FIXME" in line:
                    fixme += 1
        except Exception:
            pass

    manual = len(snapshot.warnings or [])
    manual += len(snapshot.redactions or [])
    if snapshot.users_groups and snapshot.users_groups.ssh_authorized_keys_refs:
        manual += len(snapshot.users_groups.ssh_authorized_keys_refs)

    return {"automatic": automatic, "fixme": fixme, "manual": manual}

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
        _add("Packages added", len(snapshot.rpm.packages_added), "packages", "automatic")
        _add("New from base image", len(snapshot.rpm.base_image_only), "packages", "automatic")
    if snapshot.services:
        n = len(snapshot.services.enabled_units) + len(snapshot.services.disabled_units)
        _add("Services enabled/disabled", n, "services", "automatic")
    if snapshot.config:
        _add("Config files", len(snapshot.config.files), "config", "automatic")
    if snapshot.containers and snapshot.containers.quadlet_units:
        _add("Quadlet units", len(snapshot.containers.quadlet_units), "containers", "automatic")
    if snapshot.users_groups:
        n = len(snapshot.users_groups.users or []) + len(snapshot.users_groups.groups or [])
        _add("Users/groups", n, "users_groups", "automatic")
    if snapshot.scheduled_tasks:
        n = len(snapshot.scheduled_tasks.generated_timer_units or [])
        n += len([t for t in snapshot.scheduled_tasks.systemd_timers if t.source == "local"])
        _add("Cron-to-timer conversions", n, "scheduled_tasks", "automatic")
    if snapshot.network and snapshot.network.firewall_zones:
        _add("Firewall zones", len(snapshot.network.firewall_zones), "network", "automatic")

    # Needs review
    fixme = 0
    cf = output_dir / "Containerfile"
    if cf.exists():
        try:
            for line in cf.read_text().splitlines():
                if line.strip().startswith("#") and "FIXME" in line:
                    fixme += 1
        except Exception:
            pass
    _add("Containerfile FIXMEs", fixme, "containerfile", "fixme")

    # Manual
    _add("Secrets redacted", len(snapshot.redactions or []), "secrets", "manual")
    _add("Warnings", len(snapshot.warnings or []), "warnings", "manual")
    if snapshot.users_groups and snapshot.users_groups.ssh_authorized_keys_refs:
        _add("SSH key references", len(snapshot.users_groups.ssh_authorized_keys_refs), "users_groups", "manual")

    return items

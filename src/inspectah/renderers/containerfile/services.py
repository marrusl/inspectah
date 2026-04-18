"""Containerfile section: service enablement and systemd drop-in overrides."""

from pathlib import Path

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for services and drop-ins."""
    lines: list[str] = []

    # Units whose files come from the config tree (local timers, generated timers)
    # are not yet present at this point in the build — they arrive via COPY in
    # the Scheduled Tasks section.  Exclude them here; they'll be enabled there.
    _config_tree_units: set = set()
    st_early = snapshot.scheduled_tasks
    if st_early:
        for t in st_early.systemd_timers:
            if t.source == "local" and t.name:
                _config_tree_units.add(f"{t.name}.timer")
                _config_tree_units.add(f"{t.name}.service")
        for u in st_early.generated_timer_units:
            if u.include and u.name:
                _config_tree_units.add(f"{u.name}.timer")
                _config_tree_units.add(f"{u.name}.service")

    if snapshot.services:
        enabled = snapshot.services.enabled_units
        disabled = snapshot.services.disabled_units
        if enabled or disabled:
            lines.append("# === Service Enablement ===")

            # Build unit -> owning_package lookup
            unit_owner: dict = {}
            for sc in snapshot.services.state_changes:
                if sc.owning_package:
                    unit_owner[sc.unit] = sc.owning_package

            # Build set of packages that will be present in the image
            installable: set = set()
            if snapshot.rpm:
                installable.update(snapshot.rpm.baseline_package_names or [])
                if snapshot.rpm.leaf_packages is not None:
                    installable.update(snapshot.rpm.leaf_packages)
                    if snapshot.rpm.leaf_dep_tree:
                        for deps in snapshot.rpm.leaf_dep_tree.values():
                            installable.update(deps)
                elif snapshot.rpm.packages_added:
                    installable.update(p.name for p in snapshot.rpm.packages_added if p.include)

            def _unit_installable(unit: str) -> bool:
                owner = unit_owner.get(unit)
                return owner is None or not installable or owner in installable

            safe_enabled = [u for u in enabled
                            if _sanitize_shell_value(u, "systemctl enable") is not None
                            and u not in _config_tree_units]
            safe_disabled = [u for u in disabled if _sanitize_shell_value(u, "systemctl disable") is not None]
            deferred = [u for u in enabled if u in _config_tree_units]

            orphan_enabled = [u for u in safe_enabled if not _unit_installable(u)]
            orphan_disabled = [u for u in safe_disabled if not _unit_installable(u)]
            safe_enabled = [u for u in safe_enabled if _unit_installable(u)]
            safe_disabled = [u for u in safe_disabled if _unit_installable(u)]

            unsafe_count = (len(enabled) - len(safe_enabled) - len(deferred) - len(orphan_enabled)) \
                         + (len(disabled) - len(safe_disabled) - len(orphan_disabled))
            if unsafe_count > 0:
                lines.append(f"# FIXME: {unsafe_count} unit name(s) contained unsafe characters and were skipped")
            if deferred:
                lines.append(f"# Note: {len(deferred)} config-tree unit(s) enabled later in Scheduled Tasks: "
                             + ", ".join(deferred))
            lines.append(f"# Detected: {len(safe_enabled)} non-default enabled, {len(safe_disabled)} disabled")
            if safe_enabled:
                lines.append("RUN systemctl enable " + " ".join(safe_enabled))
            if safe_disabled:
                lines.append("RUN systemctl disable " + " ".join(safe_disabled))
            for u in orphan_enabled:
                lines.append(f"# {u} — skipped (package {unit_owner[u]} not in dnf install line)")
            for u in orphan_disabled:
                lines.append(f"# {u} — skipped (package {unit_owner[u]} not in dnf install line)")
            lines.append("")

    # Systemd drop-in overrides
    if snapshot.services and snapshot.services.drop_ins:
        included_dropins = [d for d in snapshot.services.drop_ins if d.include]
        if included_dropins:
            lines.append("# === Systemd Drop-in Overrides ===")
            lines.append(f"# Detected: {len(included_dropins)} drop-in override(s) — included in COPY config/etc/ below")
            seen_dirs: set = set()
            for di in included_dropins:
                dropin_dir = str(Path(di.path).parent)
                if dropin_dir not in seen_dirs:
                    seen_dirs.add(dropin_dir)
                    lines.append(f"#   {di.unit}: {dropin_dir}/")
            lines.append("")

    return lines

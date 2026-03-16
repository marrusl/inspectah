"""Core fleet merge engine — union with prevalence filtering."""

import hashlib
from collections import defaultdict
from datetime import datetime, timezone

from ..schema import (
    InspectionSnapshot, FleetPrevalence, FleetMeta,
    RpmSection, ConfigSection, ServiceSection, NetworkSection,
    ScheduledTaskSection, ContainerSection, UserGroupSection,
    PackageEntry, RepoFile, ConfigFileEntry,
    ServiceStateChange, SystemdDropIn,
    FirewallZone, GeneratedTimerUnit, CronJob,
    QuadletUnit, ComposeFile,
)


def _prevalence_include(count: int, total: int, min_prevalence: int) -> bool:
    """Return True if count/total meets the min_prevalence threshold."""
    return (count * 100) >= (min_prevalence * total)


def _content_hash(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def _merge_identity_items(
    all_items: list[list],
    key_fn,
    total: int,
    min_prevalence: int,
    host_names: list[str],
) -> list:
    """Merge items keyed by identity only (not content)."""
    seen: dict[str, dict] = {}  # key -> {"item": model, "hosts": [hostname]}
    for snapshot_idx, items in enumerate(all_items):
        hostname = host_names[snapshot_idx]
        for item in items:
            k = key_fn(item)
            if k not in seen:
                seen[k] = {"item": item, "hosts": [hostname]}
            else:
                seen[k]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"].model_copy()
        count = len(entry["hosts"])
        item.fleet = FleetPrevalence(count=count, total=total, hosts=entry["hosts"])
        if hasattr(item, "include"):
            item.include = _prevalence_include(count, total, min_prevalence)
        result.append(item)
    return result


def _merge_content_items(
    all_items: list[list],
    identity_fn,
    variant_fn,
    total: int,
    min_prevalence: int,
    host_names: list[str],
) -> list:
    """Merge items with content variants — each (identity, variant) pair is separate."""
    seen: dict[tuple[str, str], dict] = {}
    for snapshot_idx, items in enumerate(all_items):
        hostname = host_names[snapshot_idx]
        for item in items:
            ik = identity_fn(item)
            vk = variant_fn(item)
            key = (ik, vk)
            if key not in seen:
                seen[key] = {"item": item, "hosts": [hostname]}
            else:
                seen[key]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"].model_copy()
        count = len(entry["hosts"])
        item.fleet = FleetPrevalence(count=count, total=total, hosts=entry["hosts"])
        if hasattr(item, "include"):
            item.include = _prevalence_include(count, total, min_prevalence)
        result.append(item)
    return result


def _deduplicate_strings(all_lists: list[list[str]]) -> list[str]:
    """Union of string lists, preserving first-seen order."""
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            if item not in seen:
                seen.add(item)
                result.append(item)
    return result


def _deduplicate_optional_strings(raw: list[list[str] | None]) -> list[str] | None:
    """Union of Optional string lists, preserving None when all inputs are None."""
    non_none = [v for v in raw if v is not None]
    if not non_none:
        return None
    return _deduplicate_strings(non_none)


def _deduplicate_dicts(
    all_lists: list[list[dict]],
    key_field: str,
    total: int,
    host_names: list[str],
) -> list[dict]:
    """Deduplicate dicts by a key field, inject fleet prevalence as dict key."""
    seen: dict[str, dict] = {}
    for snapshot_idx, items in enumerate(all_lists):
        hostname = host_names[snapshot_idx]
        for item in items:
            k = item.get(key_field, "")
            if k not in seen:
                seen[k] = {"item": dict(item), "hosts": [hostname]}
            else:
                seen[k]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"]
        item["fleet"] = {"count": len(entry["hosts"]), "total": total}
        result.append(item)
    return result


def _deduplicate_warning_dicts(all_lists: list[list[dict]]) -> list[dict]:
    """Deduplicate warning/redaction dicts by (source, message) tuple."""
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            key = (item.get("source", ""), item.get("message", ""))
            if key not in seen:
                seen.add(key)
                result.append(item)
    return result


def _collect_section_lists(snapshots, section_attr, list_attr):
    """Collect a list field from each snapshot's section, returning [] for missing."""
    result = []
    for s in snapshots:
        section = getattr(s, section_attr, None)
        if section is not None:
            result.append(getattr(section, list_attr, []) or [])
        else:
            result.append([])
    return result


def _merge_dep_trees(all_trees: list[dict | None]) -> dict | None:
    """Merge leaf_dep_tree dicts from multiple snapshots."""
    merged: dict[str, list[str]] = {}
    for tree in all_trees:
        if not tree:
            continue
        for leaf, deps in tree.items():
            if leaf not in merged:
                merged[leaf] = list(deps)
            else:
                existing = set(merged[leaf])
                for d in deps:
                    if d not in existing:
                        merged[leaf].append(d)
                        existing.add(d)
    return merged if merged else None


def merge_snapshots(
    snapshots: list[InspectionSnapshot],
    min_prevalence: int = 100,
    fleet_name: str = "fleet-merged",
    include_hosts: bool = True,
) -> InspectionSnapshot:
    """Merge N snapshots into a single fleet snapshot with prevalence metadata."""
    total = len(snapshots)
    host_names = [s.meta.get("hostname", f"host-{i}") for i, s in enumerate(snapshots)]

    # --- RPM ---
    rpm_section = None
    has_rpm = any(s.rpm for s in snapshots)
    if has_rpm:
        packages_added = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "packages_added"),
            key_fn=lambda p: p.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        base_image_only = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "base_image_only"),
            key_fn=lambda p: p.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        repo_files = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "repo_files"),
            key_fn=lambda r: r.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        gpg_keys = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "gpg_keys"),
            key_fn=lambda r: r.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        dnf_removed = _deduplicate_strings(
            _collect_section_lists(snapshots, "rpm", "dnf_history_removed")
        )
        # Pass-through fields from first snapshot with rpm
        first_rpm = next(s.rpm for s in snapshots if s.rpm)
        # These three fields are set together: all None or all populated.
        # Collect raw Optional values to preserve None when no snapshot has leaf data.
        raw_leaf = [(s.rpm.leaf_packages if s.rpm else None) for s in snapshots]
        raw_auto = [(s.rpm.auto_packages if s.rpm else None) for s in snapshots]
        raw_dep_trees = [(s.rpm.leaf_dep_tree if s.rpm else None) for s in snapshots]
        rpm_section = RpmSection(
            packages_added=packages_added,
            base_image_only=base_image_only,
            repo_files=repo_files,
            gpg_keys=gpg_keys,
            dnf_history_removed=dnf_removed,
            base_image=first_rpm.base_image,
            baseline_package_names=first_rpm.baseline_package_names,
            no_baseline=first_rpm.no_baseline,
            leaf_packages=_deduplicate_optional_strings(raw_leaf),
            auto_packages=_deduplicate_optional_strings(raw_auto),
            leaf_dep_tree=_merge_dep_trees(raw_dep_trees),
        )

    # --- Config ---
    config_section = None
    has_config = any(s.config for s in snapshots)
    if has_config:
        files = _merge_content_items(
            _collect_section_lists(snapshots, "config", "files"),
            identity_fn=lambda f: f.path,
            variant_fn=lambda f: _content_hash(f.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        config_section = ConfigSection(files=files)

    # --- Services ---
    services_section = None
    has_services = any(s.services for s in snapshots)
    if has_services:
        state_changes = _merge_identity_items(
            _collect_section_lists(snapshots, "services", "state_changes"),
            key_fn=lambda sc: f"{sc.unit}:{sc.action}",
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        drop_ins = _merge_content_items(
            _collect_section_lists(snapshots, "services", "drop_ins"),
            identity_fn=lambda d: d.path,
            variant_fn=lambda d: _content_hash(d.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        enabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "enabled_units")
        )
        disabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "disabled_units")
        )
        services_section = ServiceSection(
            state_changes=state_changes,
            drop_ins=drop_ins,
            enabled_units=enabled_units,
            disabled_units=disabled_units,
        )

    # --- Network (firewall zones only) ---
    network_section = None
    has_network = any(s.network for s in snapshots)
    if has_network:
        firewall_zones = _merge_identity_items(
            _collect_section_lists(snapshots, "network", "firewall_zones"),
            key_fn=lambda z: z.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        network_section = NetworkSection(firewall_zones=firewall_zones)

    # --- Scheduled Tasks ---
    sched_section = None
    has_sched = any(s.scheduled_tasks for s in snapshots)
    if has_sched:
        gen_timers = _merge_identity_items(
            _collect_section_lists(snapshots, "scheduled_tasks", "generated_timer_units"),
            key_fn=lambda t: t.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        cron_jobs = _merge_identity_items(
            _collect_section_lists(snapshots, "scheduled_tasks", "cron_jobs"),
            key_fn=lambda c: c.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        systemd_timers_all = _collect_section_lists(snapshots, "scheduled_tasks", "systemd_timers")
        timer_seen: dict[str, object] = {}
        for items in systemd_timers_all:
            for t in items:
                if t.name not in timer_seen:
                    timer_seen[t.name] = t
        sched_section = ScheduledTaskSection(
            generated_timer_units=gen_timers,
            cron_jobs=cron_jobs,
            systemd_timers=list(timer_seen.values()),
        )

    # --- Containers ---
    containers_section = None
    has_containers = any(s.containers for s in snapshots)
    if has_containers:
        quadlet_units = _merge_content_items(
            _collect_section_lists(snapshots, "containers", "quadlet_units"),
            identity_fn=lambda q: q.path,
            variant_fn=lambda q: _content_hash(q.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        compose_files = _merge_content_items(
            _collect_section_lists(snapshots, "containers", "compose_files"),
            identity_fn=lambda c: c.path,
            variant_fn=lambda c: _content_hash(
                str(sorted((img.service, img.image) for img in c.images))
            ),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        containers_section = ContainerSection(
            quadlet_units=quadlet_units,
            compose_files=compose_files,
        )

    # --- Users/Groups ---
    ug_section = None
    has_ug = any(s.users_groups for s in snapshots)
    if has_ug:
        users = _deduplicate_dicts(
            _collect_section_lists(snapshots, "users_groups", "users"),
            key_field="name", total=total, host_names=host_names,
        )
        groups = _deduplicate_dicts(
            _collect_section_lists(snapshots, "users_groups", "groups"),
            key_field="name", total=total, host_names=host_names,
        )
        sudoers = _deduplicate_strings(
            _collect_section_lists(snapshots, "users_groups", "sudoers_rules")
        )
        ug_section = UserGroupSection(
            users=users,
            groups=groups,
            sudoers_rules=sudoers,
        )

    # --- Kernel/Boot (first-snapshot pass-through) ---
    # locale and timezone use first-wins. No deep merge yet.
    # TODO: union merge alternatives when kernel_boot gets full fleet support
    kernel_boot_section = next(
        (s.kernel_boot for s in snapshots if s.kernel_boot), None,
    )

    # --- Warnings / Redactions ---
    warnings_merged = _deduplicate_warning_dicts(
        [s.warnings for s in snapshots]
    )
    redactions_merged = _deduplicate_warning_dicts(
        [s.redactions for s in snapshots]
    )

    # --- Fleet metadata ---
    fleet_meta = FleetMeta(
        source_hosts=host_names,
        total_hosts=total,
        min_prevalence=min_prevalence,
    )

    # --- Build merged snapshot ---
    first = snapshots[0]
    merged = InspectionSnapshot(
        meta={
            "hostname": fleet_name,
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "fleet": fleet_meta.model_dump(),
        },
        os_release=first.os_release,
        rpm=rpm_section,
        config=config_section,
        services=services_section,
        network=network_section,
        scheduled_tasks=sched_section,
        containers=containers_section,
        kernel_boot=kernel_boot_section,
        users_groups=ug_section,
        warnings=warnings_merged,
        redactions=redactions_merged,
    )

    if not include_hosts:
        _strip_host_lists(merged)

    return merged


def _strip_host_lists(snapshot: InspectionSnapshot) -> None:
    """Remove per-item host lists from fleet metadata (privacy mode)."""
    for section_name in ["rpm", "config", "services", "network",
                         "scheduled_tasks", "containers"]:
        section = getattr(snapshot, section_name, None)
        if section is None:
            continue
        for field_name in type(section).model_fields:
            items = getattr(section, field_name, None)
            if not isinstance(items, list):
                continue
            for item in items:
                if hasattr(item, "fleet") and item.fleet is not None:
                    item.fleet.hosts = []

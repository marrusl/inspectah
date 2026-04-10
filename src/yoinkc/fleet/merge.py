"""Core fleet merge engine — union with prevalence filtering."""

import hashlib
from collections import defaultdict
from datetime import datetime, timezone

from ..schema import (
    InspectionSnapshot, FleetPrevalence, FleetMeta,
    RpmSection, ConfigSection, ServiceSection, NetworkSection,
    ScheduledTaskSection, ContainerSection, UserGroupSection,
    NonRpmSoftwareSection, SelinuxSection,
    PackageEntry, RepoFile, ConfigFileEntry,
    ServiceStateChange, SystemdDropIn,
    FirewallZone, GeneratedTimerUnit, CronJob,
    QuadletUnit, ComposeFile,
)
from .loader import assign_display_names


def _prevalence_include(count: int, total: int, min_prevalence: int) -> bool:
    """Return True if count/total meets the min_prevalence threshold."""
    return (count * 100) >= (min_prevalence * total)


def _normalize_content(text: str) -> str:
    """Level 1 normalization: strip trailing whitespace per line, normalize line endings."""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    return "\n".join(line.rstrip() for line in lines)


def _content_hash(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


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


def _merge_module_streams(
    all_items: list[list],
    total: int,
    min_prevalence: int,
    host_names: list[str],
) -> list:
    """Merge module streams: key is module_name:stream, union profiles across hosts."""
    seen: dict[str, dict] = {}
    for snapshot_idx, items in enumerate(all_items):
        hostname = host_names[snapshot_idx]
        for item in items:
            k = f"{item.module_name}:{item.stream}"
            if k not in seen:
                seen[k] = {"item": item, "hosts": [hostname], "profiles": set(item.profiles)}
            else:
                seen[k]["hosts"].append(hostname)
                seen[k]["profiles"].update(item.profiles)

    result = []
    for entry in seen.values():
        item = entry["item"].model_copy()
        item.profiles = sorted(entry["profiles"])
        count = len(entry["hosts"])
        item.fleet = FleetPrevalence(count=count, total=total, hosts=entry["hosts"])
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


def _auto_select_variants(items: list) -> None:
    """Post-process content-variant item lists with tie-breaking auto-selection.

    Groups items by ``path``. Within each group:
    - Single variant: always selected (``include=True``).
    - Clear winner (strictly highest ``fleet.count``): winner selected, rest deselected.
    - Tie at the top: all deselected (``include=False``).

    Items lacking ``path``, ``fleet``, or ``include`` attributes are skipped.
    """
    groups: dict[str, list] = {}
    order: list[str] = []
    for item in items:
        path = getattr(item, "path", None)
        if path is None or not hasattr(item, "fleet") or item.fleet is None:
            continue
        if not hasattr(item, "include"):
            continue
        if path not in groups:
            order.append(path)
            groups[path] = []
        groups[path].append(item)

    for path in order:
        variants = groups[path]
        if len(variants) == 1:
            variants[0].include = True
            continue
        variants.sort(key=lambda v: v.fleet.count, reverse=True)
        if variants[0].fleet.count == variants[1].fleet.count:
            for v in variants:
                v.include = False
        else:
            variants[0].include = True
            for v in variants[1:]:
                v.include = False


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
        item["fleet"] = {
            "count": len(entry["hosts"]),
            "total": total,
            "hosts": list(entry["hosts"]),
        }
        result.append(item)
    return result


def _deduplicate_warning_dicts(all_lists: list[list]) -> list:
    """Deduplicate warning/redaction items by identity key.

    Supports both plain dicts (warnings) and RedactionFinding objects (redactions).
    """
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            if isinstance(item, dict):
                key = (item.get("path", ""), item.get("pattern", ""),
                       item.get("source", ""), item.get("message", ""),
                       item.get("line", ""))
            else:
                # RedactionFinding — key on (path, pattern, source, replacement)
                # Including replacement prevents collapsing distinct inline findings
                # in the same file (e.g. REDACTED_PASSWORD_1 vs REDACTED_PASSWORD_2).
                key = (getattr(item, "path", ""), getattr(item, "pattern", ""), getattr(item, "source", ""), getattr(item, "replacement", None))
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
    full_hostnames = [s.meta.get("hostname", f"host-{i}") for i, s in enumerate(snapshots)]
    host_names = assign_display_names(snapshots)

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
        module_streams = _merge_module_streams(
            _collect_section_lists(snapshots, "rpm", "module_streams"),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        version_locks = _merge_content_items(
            _collect_section_lists(snapshots, "rpm", "version_locks"),
            identity_fn=lambda e: f"{e.name}.{e.arch}",
            variant_fn=lambda e: f"{e.epoch}:{e.version}-{e.release}",
            total=total, min_prevalence=min_prevalence, host_names=host_names,
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
            module_streams=module_streams,
            version_locks=version_locks,
            base_image=first_rpm.base_image,
            baseline_package_names=first_rpm.baseline_package_names,
            baseline_module_streams=first_rpm.baseline_module_streams,
            no_baseline=first_rpm.no_baseline,
            leaf_packages=_deduplicate_optional_strings(raw_leaf),
            auto_packages=_deduplicate_optional_strings(raw_auto),
            leaf_dep_tree=_merge_dep_trees(raw_dep_trees),
        )
        if rpm_section.baseline_module_streams:
            module_stream_conflicts = []
            for ms in rpm_section.module_streams:
                if ms.baseline_match:
                    continue
                base_stream = rpm_section.baseline_module_streams.get(ms.module_name)
                if base_stream is not None and base_stream != ms.stream:
                    module_stream_conflicts.append(
                        f"{ms.module_name}: host={ms.stream}, base_image={base_stream}"
                    )
            rpm_section.module_stream_conflicts = module_stream_conflicts

    # --- Config ---
    config_section = None
    has_config = any(s.config for s in snapshots)
    if has_config:
        files = _merge_content_items(
            _collect_section_lists(snapshots, "config", "files"),
            identity_fn=lambda f: f.path,
            variant_fn=lambda f: _content_hash(_normalize_content(f.content)),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        _auto_select_variants(files)
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
            variant_fn=lambda d: _content_hash(_normalize_content(d.content)),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        enabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "enabled_units")
        )
        disabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "disabled_units")
        )
        _auto_select_variants(drop_ins)
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
            variant_fn=lambda q: _content_hash(_normalize_content(q.content)),
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
        _auto_select_variants(quadlet_units)
        _auto_select_variants(compose_files)
        containers_section = ContainerSection(
            quadlet_units=quadlet_units,
            compose_files=compose_files,
        )

    # --- Non-RPM Software ---
    non_rpm_section = None
    has_non_rpm = any(s.non_rpm_software for s in snapshots)
    if has_non_rpm:
        non_rpm_items = _merge_identity_items(
            _collect_section_lists(snapshots, "non_rpm_software", "items"),
            key_fn=lambda i: i.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        non_rpm_env_files = _merge_content_items(
            _collect_section_lists(snapshots, "non_rpm_software", "env_files"),
            identity_fn=lambda f: f.path,
            variant_fn=lambda f: _content_hash(_normalize_content(f.content)),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        _auto_select_variants(non_rpm_env_files)
        non_rpm_section = NonRpmSoftwareSection(
            items=non_rpm_items,
            env_files=non_rpm_env_files,
        )

    # --- SELinux ---
    selinux_section = None
    has_selinux = any(s.selinux for s in snapshots)
    if has_selinux:
        port_labels = _merge_identity_items(
            _collect_section_lists(snapshots, "selinux", "port_labels"),
            key_fn=lambda p: f"{p.protocol}/{p.port}",
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        custom_modules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "custom_modules"),
        )
        fcontext_rules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "fcontext_rules"),
        )
        audit_rules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "audit_rules"),
        )
        pam_configs = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "pam_configs"),
        )
        boolean_overrides = _deduplicate_dicts(
            _collect_section_lists(snapshots, "selinux", "boolean_overrides"),
            key_field="name",
            total=total, host_names=host_names,
        )
        first_se = next(s.selinux for s in snapshots if s.selinux)
        selinux_section = SelinuxSection(
            mode=first_se.mode,
            fips_mode=first_se.fips_mode,
            port_labels=port_labels,
            custom_modules=custom_modules,
            fcontext_rules=fcontext_rules,
            audit_rules=audit_rules,
            pam_configs=pam_configs,
            boolean_overrides=boolean_overrides,
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
            "fleet": {
                **fleet_meta.model_dump(),
                "host_title_map": dict(zip(host_names, full_hostnames)),
            },
        },
        os_release=first.os_release,
        rpm=rpm_section,
        config=config_section,
        services=services_section,
        network=network_section,
        scheduled_tasks=sched_section,
        containers=containers_section,
        non_rpm_software=non_rpm_section,
        selinux=selinux_section,
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
                         "scheduled_tasks", "containers",
                         "selinux", "non_rpm_software", "users_groups"]:
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
                elif isinstance(item, dict) and "fleet" in item:
                    fleet = item["fleet"]
                    if isinstance(fleet, dict) and "hosts" in fleet:
                        fleet["hosts"] = []

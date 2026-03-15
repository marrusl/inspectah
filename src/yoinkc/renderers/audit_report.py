"""Markdown audit report renderer."""

from pathlib import Path

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot
from ._triage import compute_triage, _config_file_count, _QUADLET_PREFIX


def _storage_recommendation(mount_point: str, fstype: str, device: str) -> str:
    """Map a mount point to a migration recommendation."""
    mp = mount_point
    fs = fstype.lower()

    if mp in ("/", "/boot", "/boot/efi"):
        return "image-embedded (managed by bootc)"
    if "nfs" in fs or "cifs" in fs or "glusterfs" in fs or "9p" in fs:
        return "external storage — keep as network mount"
    if "swap" in fs or mp == "swap" or mp == "none":
        return "swap — configure via kernel args or systemd"
    if mp == "/tmp" or mp == "/dev/shm":
        return "tmpfs — ephemeral, no action"
    if mp.startswith("/var/lib/mysql") or mp.startswith("/var/lib/pgsql") or mp.startswith("/var/lib/mongodb"):
        return "PVC / volume mount — database storage, must persist"
    if mp.startswith("/var/lib/containers") or mp.startswith("/var/lib/docker"):
        return "PVC / volume mount — container storage"
    if mp.startswith("/var/lib") or mp.startswith("/var/data"):
        return "PVC / volume mount — application data"
    if mp.startswith("/var/log"):
        return "PVC / volume mount — log retention"
    if mp.startswith("/var"):
        return "PVC / volume mount — mutable state"
    if mp.startswith("/home"):
        return "PVC / volume mount — user home directories"
    if mp.startswith("/opt"):
        return "PVC or image-embedded — review application needs"
    if mp.startswith("/srv"):
        return "PVC / volume mount — served content"
    if mp.startswith("/mnt") or mp.startswith("/media"):
        return "external storage — removable/temporary mount"
    return "review — determine if data is mutable or static"


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    output_dir = Path(output_dir)
    lines = ["# Audit Report", ""]
    if snapshot.os_release:
        lines.append(f"**OS:** {snapshot.os_release.pretty_name or snapshot.os_release.name}")
        lines.append("")
    lines.append("## Executive Summary")
    lines.append("")
    n_added = len(snapshot.rpm.packages_added) if snapshot.rpm else 0
    n_base_only = len(snapshot.rpm.base_image_only) if snapshot.rpm else 0
    no_baseline = getattr(snapshot.rpm, "no_baseline", False) if snapshot.rpm else False
    n_config = _config_file_count(snapshot)
    n_redactions = len(snapshot.redactions)
    n_containers = 0
    if snapshot.containers:
        n_containers = len(snapshot.containers.quadlet_units or []) + len(snapshot.containers.compose_files or [])

    triage = compute_triage(snapshot, output_dir)
    lines.append(f"**{triage['automatic']}** items handled automatically &nbsp;|&nbsp; "
                 f"**{triage['fixme']}** items with FIXME (need review) &nbsp;|&nbsp; "
                 f"**{triage['manual']}** items need manual intervention")
    lines.append("")
    if no_baseline:
        lines.append(f"- Packages (no baseline — all installed): {n_added}")
    else:
        lines.append(f"- Packages added (beyond base image): {n_added}")
    lines.append(f"- Packages in target image only: {n_base_only}")
    lines.append(f"- Config files captured: {n_config}")
    lines.append(f"- Containers/quadlet found: {n_containers}")
    lines.append(f"- Secrets redacted: {n_redactions}")
    lines.append("")

    if snapshot.rpm:
        lines.append("## RPM / Packages")
        lines.append("")
        if snapshot.rpm.no_baseline:
            lines.append("*No baseline — showing all packages.*")
            lines.append("")
        elif snapshot.rpm.baseline_package_names:
            base_img = snapshot.rpm.base_image or "target base image"
            lines.append(f"Baseline: {len(snapshot.rpm.baseline_package_names)} packages from `{base_img}`.")
            lines.append("")
        if snapshot.rpm.leaf_packages is not None:
            leaf_set = set(snapshot.rpm.leaf_packages)
            auto_set = set(snapshot.rpm.auto_packages or [])
            dep_tree = snapshot.rpm.leaf_dep_tree or {}
            leaf_pkgs = [p for p in snapshot.rpm.packages_added if p.name in leaf_set]
            auto_pkgs = [p for p in snapshot.rpm.packages_added if p.name in auto_set]
            lines.append(f"### Explicitly installed ({len(leaf_pkgs)})")
            lines.append("")
            lines.append("These packages appear in the Containerfile `dnf install` line.")
            lines.append("")
            # Group by source repo
            _leaf_by_repo: dict = {}
            for p in leaf_pkgs:
                repo = p.source_repo or "(unknown)"
                _leaf_by_repo.setdefault(repo, []).append(p)
            _sorted_repos = sorted(k for k in _leaf_by_repo if k != "(unknown)")
            if "(unknown)" in _leaf_by_repo:
                _sorted_repos.append("(unknown)")
            for repo in _sorted_repos:
                rpkgs = _leaf_by_repo[repo]
                lines.append(f"#### {repo} ({len(rpkgs)})")
                lines.append("")
                for p in rpkgs:
                    prefix = "[EXCLUDED] " if not p.include else ""
                    lines.append(f"- {prefix}{p.name} {p.version}-{p.release}.{p.arch}")
                lines.append("")
        else:
            lines.append("### Added")
            for p in snapshot.rpm.packages_added:
                prefix = "[EXCLUDED] " if not p.include else ""
                lines.append(f"- {prefix}{p.name} {p.version}-{p.release}.{p.arch}")
            lines.append("")
        if snapshot.rpm.base_image_only:
            lines.append("### In target image only (not on inspected host)")
            for p in snapshot.rpm.base_image_only:
                lines.append(f"- {p.name}")
            lines.append("")
        if snapshot.rpm.version_changes:
            lines.append("### Version Changes")
            lines.append("")
            downgrades = [vc for vc in snapshot.rpm.version_changes
                          if vc.direction.value == "downgrade"]
            upgrades = [vc for vc in snapshot.rpm.version_changes
                        if vc.direction.value == "upgrade"]
            if downgrades:
                lines.append(f"**{len(downgrades)} downgrade(s)** — base image has older version than host:")
                lines.append("")
                for vc in downgrades:
                    lines.append(f"- [WARNING] **{vc.name}** ({vc.arch}): {vc.host_version} → {vc.base_version}")
                lines.append("")
            if upgrades:
                lines.append(f"**{len(upgrades)} upgrade(s)** — base image has newer version than host:")
                lines.append("")
                for vc in upgrades:
                    lines.append(f"- {vc.name} ({vc.arch}): {vc.host_version} → {vc.base_version}")
                lines.append("")
        if snapshot.rpm.rpm_va:
            lines.append("### Modified file details (rpm -Va)")
            for e in snapshot.rpm.rpm_va:
                lines.append(f"- `{e.path}` ({e.flags})")
            lines.append("")
        if snapshot.rpm.dnf_history_removed:
            lines.append("### Previously installed then removed (dnf history)")
            lines.append("")
            lines.append("These packages were installed and later removed. They may have left behind config files or state.")
            lines.append("")
            for name in snapshot.rpm.dnf_history_removed:
                lines.append(f"- {name}")
            lines.append("")

    if snapshot.services and snapshot.services.state_changes:
        service_rows = [s for s in snapshot.services.state_changes if s.action != "unchanged"]
        if service_rows:
            lines.append("## Services")
            lines.append("")
            lines.append("| Unit | Current | Default | Action |")
            lines.append("|------|---------|---------|--------|")
            for s in service_rows:
                prefix = "[EXCLUDED] " if not s.include else ""
                lines.append(f"| {prefix}{s.unit} | {s.current_state} | {s.default_state} | {s.action} |")
            lines.append("")

    if snapshot.services and snapshot.services.drop_ins:
        included = [d for d in snapshot.services.drop_ins if d.include]
        excluded = [d for d in snapshot.services.drop_ins if not d.include]
        if included or excluded:
            lines.append("### Systemd drop-in overrides")
            lines.append("")
            for di in included:
                lines.append(f"**{di.unit}** — `{di.path}`")
                if di.content.strip():
                    lines.append("```ini")
                    lines.append(di.content.strip())
                    lines.append("```")
                lines.append("")
            for di in excluded:
                lines.append(f"[EXCLUDED] **{di.unit}** — `{di.path}`")
                lines.append("")

    config_files = [
        f for f in (snapshot.config.files if snapshot.config and snapshot.config.files else [])
        if not f.path.lstrip("/").startswith(_QUADLET_PREFIX)
    ]
    if config_files:
        lines.append("## Configuration Files")
        lines.append("")
        modified = [f for f in config_files if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        unowned = [f for f in config_files if f.kind == ConfigFileKind.UNOWNED]
        orphaned = [f for f in config_files if f.kind == ConfigFileKind.ORPHANED]
        lines.append(f"- RPM-owned modified: {len(modified)}")
        lines.append(f"- Unowned: {len(unowned)}")
        if orphaned:
            lines.append(f"- Orphaned (from removed packages): {len(orphaned)}")
        ca_anchors = [f for f in config_files
                      if f.include and f.path.lstrip("/").startswith("etc/pki/ca-trust/source/anchors/")]
        if ca_anchors:
            names = ", ".join(f"`{f.path}`" for f in ca_anchors)
            lines.append(f"- **Custom CA certificates** ({len(ca_anchors)}): {names}")
            lines.append("  `update-ca-trust` will be run in the Containerfile after COPYing these files.")
        for f in config_files:
            prefix = "[EXCLUDED] " if not f.include else ""
            flags_note = f" — rpm -Va flags: `{f.rpm_va_flags}`" if f.rpm_va_flags else ""
            pkg_note = f" (package: {f.package})" if f.package else ""
            lines.append(f"- {prefix}`{f.path}` ({f.kind.value}{flags_note}{pkg_note})")
            if f.diff_against_rpm and f.diff_against_rpm.strip():
                lines.append("  Diff against RPM default:")
                lines.append("```diff")
                lines.append(f.diff_against_rpm.strip())
                lines.append("```")
                lines.append("")
        lines.append("")

    net = snapshot.network
    has_network = net and (
        net.connections or net.firewall_zones or net.firewall_direct_rules
        or net.static_routes or net.ip_routes or net.ip_rules
        or net.proxy or net.hosts_additions or net.resolv_provenance
    )
    if has_network:
        lines.append("## Network")
        lines.append("")

        if net.connections:
            static_conns = [c for c in net.connections if c.method == "static"]
            dhcp_conns = [c for c in net.connections if c.method == "dhcp"]
            other_conns = [c for c in net.connections if c.method not in ("static", "dhcp")]

            lines.append("### Connections")
            lines.append("")
            if static_conns:
                lines.append("**Static (bake into image):**")
                for c in static_conns:
                    lines.append(f"- `{c.name}` — {c.type} — `{c.path}`")
            if dhcp_conns:
                lines.append("**DHCP (kickstart at deploy time):**")
                for c in dhcp_conns:
                    lines.append(f"- `{c.name}` — {c.type} — `{c.path}`")
            if other_conns:
                lines.append("**Other:**")
                for c in other_conns:
                    lines.append(f"- `{c.name}` — method={c.method} — `{c.path}`")
            lines.append("")

        included_zones = [z for z in net.firewall_zones if z.include]
        if included_zones:
            lines.append("### Firewall zones (bake into image)")
            lines.append("")
            for z in included_zones:
                svc_str = ', '.join(z.services) or '—'
                port_str = ', '.join(z.ports) or '—'
                lines.append(f"**{z.name}:** services={svc_str} | ports={port_str} | rich rules={len(z.rich_rules)}")
                for r in z.rich_rules[:10]:
                    lines.append(f"  - `{r[:200]}`")
                fw_cmds = []
                for svc in z.services:
                    fw_cmds.append(f"RUN firewall-offline-cmd --zone={z.name} --add-service={svc}")
                for port in z.ports:
                    fw_cmds.append(f"RUN firewall-offline-cmd --zone={z.name} --add-port={port}")
                for rr in z.rich_rules:
                    if rr:
                        fw_cmds.append(f"RUN firewall-offline-cmd --zone={z.name} --add-rich-rule='{rr}'")
                if fw_cmds:
                    lines.append("")
                    lines.append("#### Alternative: firewall-offline-cmd (instead of COPY)")
                    lines.append("")
                    lines.append("```dockerfile")
                    lines.extend(fw_cmds)
                    lines.append("```")
                lines.append("")

        included_direct = [r for r in net.firewall_direct_rules if r.include]
        if included_direct:
            lines.append("### Firewall direct rules (bake into image)")
            lines.append("")
            for r in included_direct:
                lines.append(f"- `{r.chain}` {r.ipv} table={r.table}: `{r.args}`")
            dr_cmds = [
                f"RUN firewall-offline-cmd --direct --add-rule {dr.ipv} {dr.table} {dr.chain} {dr.priority} {dr.args}"
                for dr in included_direct
            ]
            lines.append("")
            lines.append("#### Alternative: firewall-offline-cmd (instead of COPY)")
            lines.append("")
            lines.append("```dockerfile")
            lines.extend(dr_cmds)
            lines.append("```")
            lines.append("")

        if net.resolv_provenance:
            lines.append(f"### DNS — resolv.conf provenance: **{net.resolv_provenance}**")
            lines.append("")

        if net.ip_routes:
            static_rt = [r for r in net.ip_routes if "proto static" in r]
            if static_rt:
                lines.append("### Static routes (from `ip route`)")
                lines.append("")
                for r in static_rt:
                    lines.append(f"- `{r}`")
                lines.append("")

        if net.ip_rules:
            lines.append("### Policy routing rules (non-default)")
            lines.append("")
            for r in net.ip_rules:
                lines.append(f"- `{r}`")
            lines.append("")

        if net.hosts_additions:
            lines.append("### /etc/hosts additions")
            for h in net.hosts_additions:
                lines.append(f"- `{h}`")
            lines.append("")

        if net.proxy:
            lines.append("### Proxy configuration")
            for p in net.proxy:
                lines.append(f"- {p.source}: `{p.line}`")
            lines.append("")

    if snapshot.storage and (snapshot.storage.fstab_entries or snapshot.storage.mount_points or snapshot.storage.lvm_info):
        lines.append("## Storage Migration Plan")
        lines.append("")
        lines.append("Each mount point below should be mapped to one of:")
        lines.append("- **Image-embedded** — small, static data seeded at initial bootstrap")
        lines.append("- **PVC / volume mount** — application data, databases")
        lines.append("- **External storage** — NFS/CIFS shared filesystems")
        lines.append("")
        if snapshot.storage.fstab_entries:
            lines.append("| Device | Mount Point | FS Type | Recommendation |")
            lines.append("|--------|-------------|---------|----------------|")
            for e in snapshot.storage.fstab_entries:
                rec = _storage_recommendation(e.mount_point, e.fstype, e.device)
                lines.append(f"| `{e.device}` | `{e.mount_point}` | {e.fstype} | {rec} |")
            lines.append("")
        if snapshot.storage.lvm_info:
            lines.append("### LVM Layout")
            for lv in snapshot.storage.lvm_info:
                lines.append(f"- {lv.lv_name} ({lv.vg_name}) {lv.lv_size}")
            lines.append("")
        if snapshot.storage.credential_refs:
            lines.append("### Mount Credential References")
            lines.append("")
            lines.append("These mounts reference credential files that need a secret injection strategy at deploy time.")
            lines.append("")
            lines.append("| Mount Point | Credential Path | Action |")
            lines.append("|-------------|-----------------|--------|")
            for cr in snapshot.storage.credential_refs:
                lines.append(f"| `{cr.mount_point}` | `{cr.credential_path}` | Inject via secret store or kickstart |")
            lines.append("")

    st = snapshot.scheduled_tasks
    if st and (st.cron_jobs or st.systemd_timers or st.generated_timer_units or st.at_jobs):
        lines.append("## Scheduled tasks")
        lines.append("")

        # Existing systemd timers (grouped by source)
        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        vendor_timers = [t for t in st.systemd_timers if t.source == "vendor"]
        if local_timers:
            lines.append("### Existing systemd timers (local)")
            lines.append("")
            lines.append("| Timer | Schedule | ExecStart | Path |")
            lines.append("|-------|----------|-----------|------|")
            for t in local_timers:
                lines.append(f"| {t.name} | {t.on_calendar} | `{t.exec_start}` | `{t.path}` |")
            lines.append("")
        if vendor_timers:
            lines.append(f"- {len(vendor_timers)} vendor timer(s) from the base image are present and will carry over automatically.")
            lines.append("")

        # Cron-converted timers
        if st.generated_timer_units:
            lines.append("### Cron-converted timers")
            lines.append("")
            for u in st.generated_timer_units:
                prefix = "[EXCLUDED] " if not u.include else ""
                lines.append(f"- {prefix}**{u.name}** — converted from `{u.source_path}` (cron: `{u.cron_expr}`)")
            lines.append("")

        # Raw cron jobs — only show operator-added jobs
        operator_cron = [j for j in st.cron_jobs if not j.rpm_owned]
        rpm_cron = [j for j in st.cron_jobs if j.rpm_owned]
        if operator_cron or rpm_cron:
            lines.append("### Cron jobs")
            lines.append("")
            if rpm_cron:
                lines.append(f"- {len(rpm_cron)} package-owned cron job(s) not listed (handled by package install).")
                lines.append("")
            if operator_cron:
                lines.append("| Path | Source | Action |")
                lines.append("|------|--------|--------|")
                for j in operator_cron:
                    prefix = "[EXCLUDED] " if not j.include else ""
                    lines.append(f"| {prefix}`{j.path}` | {j.source} | Convert to systemd timer |")
                lines.append("")

        # At jobs
        if st.at_jobs:
            lines.append("### At jobs")
            lines.append("")
            lines.append("| File | User | Command |")
            lines.append("|------|------|---------|")
            for a in st.at_jobs:
                cmd = a.command[:77] + "..." if len(a.command) > 80 else a.command
                lines.append(f"| `{a.file}` | {a.user} | `{cmd}` |")
            lines.append("")

    ct = snapshot.containers
    if ct and (ct.quadlet_units or ct.compose_files or ct.running_containers):
        lines.append("## Container workloads")
        lines.append("")

        if ct.quadlet_units:
            lines.append("### Quadlet units")
            lines.append("")
            lines.append("| Unit | Image | Path |")
            lines.append("|------|-------|------|")
            for u in ct.quadlet_units:
                prefix = "[EXCLUDED] " if not u.include else ""
                lines.append(f"| {prefix}{u.name} | `{u.image or '*none*'}` | `{u.path}` |")
            lines.append("")

        if ct.compose_files:
            lines.append("### Compose files")
            lines.append("")
            for c in ct.compose_files:
                prefix = "[EXCLUDED] " if not c.include else ""
                lines.append(f"{prefix}**`{c.path}`**")
                if c.images:
                    lines.append("")
                    lines.append("| Service | Image |")
                    lines.append("|---------|-------|")
                    for img in c.images:
                        lines.append(f"| {img.service} | `{img.image}` |")
                lines.append("")

        if ct.running_containers:
            lines.append("### Running containers (podman)")
            lines.append("")
            lines.append("| Name | Image | Status |")
            lines.append("|------|-------|--------|")
            for r in ct.running_containers:
                name = r.name or r.id[:12]
                lines.append(f"| {name} | `{r.image}` | {r.status} |")
            lines.append("")

            for r in ct.running_containers:
                name = r.name or r.id[:12]
                if r.mounts or r.networks or r.env:
                    lines.append(f"<details><summary>{name} details</summary>")
                    lines.append("")
                    if r.mounts:
                        lines.append("**Mounts:**")
                        for m in r.mounts:
                            rw = "rw" if m.rw else "ro"
                            lines.append(f"- `{m.source}` → `{m.destination}` ({m.type}, {rw})")
                    if r.networks:
                        lines.append("")
                        lines.append("**Networks:**")
                        for net_name, info in r.networks.items():
                            lines.append(f"- {net_name}: {info.get('ip', '')}")
                    if r.env:
                        lines.append("")
                        lines.append("**Environment:**")
                        for e in r.env:
                            if "=" in e and not e.startswith("PATH="):
                                lines.append(f"- `{e}`")
                    lines.append("")
                    lines.append("</details>")
                    lines.append("")

    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        lines.append("## Non-RPM software")
        lines.append("")

        # Group items by method category
        elf_items = [i for i in snapshot.non_rpm_software.items if i.lang]
        venv_items = [i for i in snapshot.non_rpm_software.items if i.method == "python venv"]
        git_items = [i for i in snapshot.non_rpm_software.items if i.method == "git repository"]
        pip_items = [i for i in snapshot.non_rpm_software.items if i.method == "pip dist-info"]
        other_items = [i for i in snapshot.non_rpm_software.items
                       if not i.lang and i.method not in ("python venv", "git repository", "pip dist-info")]

        if elf_items:
            lines.append("### Compiled binaries")
            lines.append("")
            lines.append("| Path | Language | Linking | Shared Libraries |")
            lines.append("|------|----------|---------|------------------|")
            for i in elf_items:
                prefix = "[EXCLUDED] " if not i.include else ""
                linking = "static" if i.static else "dynamic"
                libs = ", ".join(i.shared_libs[:5])
                if len(i.shared_libs) > 5:
                    libs += " ..."
                lines.append(f"| {prefix}`{i.path}` | {i.lang} | {linking} | {libs or '—'} |")
            lines.append("")

        if venv_items:
            lines.append("### Python virtual environments")
            lines.append("")
            for v in venv_items:
                ssp_label = "**system-site-packages**" if v.system_site_packages else "isolated"
                lines.append(f"#### `{v.path}` ({ssp_label})")
                lines.append("")
                if v.packages:
                    lines.append("| Package | Version |")
                    lines.append("|---------|---------|")
                    for p in v.packages:
                        lines.append(f"| {p.name} | {p.version} |")
                lines.append("")

        if git_items:
            lines.append("### Git-managed directories")
            lines.append("")
            lines.append("| Path | Remote | Branch | Commit |")
            lines.append("|------|--------|--------|--------|")
            for i in git_items:
                lines.append(f"| `{i.path}` | {i.git_remote} | {i.git_branch} | `{i.git_commit[:12]}` |")
            lines.append("")

        if pip_items:
            lines.append("### System pip packages")
            lines.append("")
            lines.append("| Package | Version | Path |")
            lines.append("|---------|---------|------|")
            for i in pip_items:
                lines.append(f"| {i.name} | {i.version} | `{i.path}` |")
            lines.append("")

        if other_items:
            lines.append("### Other non-RPM items")
            lines.append("")
            lines.append("| Path / Name | Version | Confidence | Method |")
            lines.append("|-------------|---------|------------|--------|")
            for i in other_items:
                pn = i.path or i.name
                ver = i.version or "—"
                conf = i.confidence or "unknown"
                method = i.method or "—"
                lines.append(f"| `{pn}` | {ver} | {conf} | {method} |")
            lines.append("")

    kb = snapshot.kernel_boot
    has_kernel = kb and (
        kb.cmdline or kb.sysctl_overrides or kb.non_default_modules
        or kb.modules_load_d or kb.modprobe_d or kb.dracut_conf
        or kb.tuned_active or kb.tuned_custom_profiles
    )
    if has_kernel:
        lines.append("## Kernel and boot")
        lines.append("")
        if kb.cmdline:
            lines.append(f"- cmdline: `{kb.cmdline}`")
        if kb.grub_defaults:
            lines.append("- GRUB defaults present")
        if kb.tuned_active:
            lines.append(f"- Tuned profile: **{kb.tuned_active}**")
        if kb.tuned_custom_profiles:
            lines.append(f"- Custom tuned profiles ({len(kb.tuned_custom_profiles)}):")
            for tp in kb.tuned_custom_profiles:
                lines.append(f"  - `{tp.path}`")

        if kb.non_default_modules:
            lines.append("")
            lines.append(f"- {len(kb.non_default_modules)} kernel module(s) loaded at inspection time (hardware-specific, not included in the image). See modules-load.d entries below for explicitly configured modules.")

        if kb.sysctl_overrides:
            lines.append("")
            lines.append(f"### Non-default sysctl values ({len(kb.sysctl_overrides)})")
            lines.append("")
            lines.append("| Key | Runtime | Default | Source |")
            lines.append("|-----|---------|---------|--------|")
            for s in kb.sysctl_overrides:
                prefix = "[EXCLUDED] " if not s.include else ""
                lines.append(f"| {prefix}`{s.key}` | **{s.runtime}** | {s.default or '—'} | `{s.source}` |")

        for m in (kb.modules_load_d or []):
            lines.append(f"- modules-load.d: `{m.path}`")
        for m in (kb.modprobe_d or []):
            lines.append(f"- modprobe.d: `{m.path}`")
        for d in (kb.dracut_conf or []):
            lines.append(f"- dracut: `{d.path}`")
        lines.append("")

    has_selinux = snapshot.selinux and (
        snapshot.selinux.mode or snapshot.selinux.custom_modules
        or snapshot.selinux.boolean_overrides or snapshot.selinux.fcontext_rules
        or snapshot.selinux.audit_rules or snapshot.selinux.fips_mode
        or snapshot.selinux.pam_configs
    )
    if has_selinux:
        lines.append("## SELinux / Security")
        lines.append("")
        if snapshot.selinux.mode:
            lines.append(f"- SELinux mode: {snapshot.selinux.mode}")
        if snapshot.selinux.fips_mode:
            lines.append("- FIPS mode: **enabled**")
        if snapshot.selinux.custom_modules:
            lines.append(f"- **Custom policy modules** ({len(snapshot.selinux.custom_modules)}):")
            for m in snapshot.selinux.custom_modules:
                lines.append(f"  - `{m}`")
        non_default_bools = [b for b in (snapshot.selinux.boolean_overrides or []) if b.get("non_default")]
        if non_default_bools:
            lines.append(f"- **Non-default booleans** ({len(non_default_bools)}):")
            for b in non_default_bools:
                name = b.get("name", "?")
                cur = b.get("current", "?")
                dflt = b.get("default", "?")
                desc = b.get("description", "")
                lines.append(f"  - `{name}` = **{cur}** (default: {dflt}) — {desc}")
        unchanged_count = len(snapshot.selinux.boolean_overrides or []) - len(non_default_bools)
        if unchanged_count > 0:
            lines.append(f"- Unchanged booleans: {unchanged_count} (at default values)")
        if snapshot.selinux.fcontext_rules:
            lines.append(f"- **Custom fcontext rules** ({len(snapshot.selinux.fcontext_rules)}):")
            for fc in snapshot.selinux.fcontext_rules:
                lines.append(f"  - `{fc}`")
        if snapshot.selinux.audit_rules:
            lines.append(f"- Audit rule files: {len(snapshot.selinux.audit_rules)}")
            for a in snapshot.selinux.audit_rules:
                lines.append(f"  - `{a}`")
        if snapshot.selinux.pam_configs:
            lines.append(f"- PAM config files: {len(snapshot.selinux.pam_configs)}")
            for p in snapshot.selinux.pam_configs:
                lines.append(f"  - `{p}`")
        lines.append("")

    has_users = snapshot.users_groups and (
        snapshot.users_groups.users or snapshot.users_groups.groups
        or snapshot.users_groups.sudoers_rules or snapshot.users_groups.ssh_authorized_keys_refs
    )
    if has_users:
        ug = snapshot.users_groups
        lines.append("## Users and groups")
        lines.append("")
        for u in (ug.users or []):
            prefix = "[EXCLUDED] " if not u.get("include", True) else ""
            shell = u.get("shell") or ""
            home = u.get("home") or ""
            lines.append(f"- {prefix}User: **{u.get('name') or ''}** (uid {u.get('uid') or ''}, home `{home}`, shell `{shell}`)")
        for g in (ug.groups or []):
            prefix = "[EXCLUDED] " if not g.get("include", True) else ""
            members = g.get("members") or []
            mem_str = f", members: {', '.join(members)}" if members else ""
            lines.append(f"- {prefix}Group: **{g.get('name') or ''}** (gid {g.get('gid') or ''}{mem_str})")
        if ug.sudoers_rules:
            lines.append("")
            lines.append("### Sudoers rules")
            for r in ug.sudoers_rules:
                lines.append(f"- `{r}`")
        if ug.ssh_authorized_keys_refs:
            lines.append("")
            lines.append("### SSH authorized_keys (flagged for manual handling)")
            for ref in ug.ssh_authorized_keys_refs:
                lines.append(f"- User **{ref.get('user') or ''}**: `{ref.get('path') or ''}`")
        lines.append("")

        # User Migration Strategy table
        if ug.users:
            ssh_users = {ref.get("user") for ref in (ug.ssh_authorized_keys_refs or [])}
            sudo_users = set()
            for rule in (ug.sudoers_rules or []):
                for u in ug.users:
                    if u.get("name", "") in rule:
                        sudo_users.add(u["name"])

            lines.append("### User Migration Strategy")
            lines.append("")
            lines.append("| User | UID | Type | Strategy | Notes |")
            lines.append("|------|-----|------|----------|-------|")
            for u in ug.users:
                name = u.get("name", "")
                uid = u.get("uid", "")
                cls = u.get("classification", "?")
                strategy = u.get("strategy", "?")
                notes = []
                shell = u.get("shell", "")
                if shell and shell != "/bin/bash":
                    notes.append(f"shell: {shell}")
                if name in sudo_users:
                    notes.append("has sudo")
                if name in ssh_users:
                    notes.append("SSH keys")
                shadow_entry = next((s for s in (ug.shadow_entries or []) if s.split(":")[0] == name), None)
                if shadow_entry:
                    pw = shadow_entry.split(":")[1] if ":" in shadow_entry else ""
                    if pw and pw not in ("!", "!!", "*"):
                        notes.append("has password")
                lines.append(f"| {name} | {uid} | {cls} | {strategy} | {', '.join(notes) if notes else ''} |")
            lines.append("")
            lines.append("**Strategies:** "
                         "sysusers = systemd-sysusers drop-in (boot-time), "
                         "useradd = explicit RUN in Containerfile, "
                         "kickstart = deferred to deploy-time provisioning, "
                         "blueprint = bootc-image-builder TOML")
            lines.append("")

    lines.append("## Data Migration Plan (/var)")
    lines.append("")
    lines.append("Content under `/var` is seeded at initial bootstrap and **not updated** by subsequent bootc deployments.")
    lines.append("`tmpfiles.d` snippets ensure expected directories exist on every boot.")
    lines.append("Review application data under `/var/lib`, `/var/log`, `/var/data` for separate migration strategies.")
    lines.append("")
    if snapshot.storage and snapshot.storage.var_directories:
        lines.append("| Directory | Size | Recommendation |")
        lines.append("|-----------|------|----------------|")
        for vd in snapshot.storage.var_directories:
            lines.append(f"| `/{vd.path}` | {vd.size_estimate} | {vd.recommendation} |")
        lines.append("")
    else:
        lines.append("*No significant application data directories found under `/var`.*")
        lines.append("")

    # --- Environment-specific considerations ---
    # Each subsection is conditional on relevant data being present.
    env_notes: list = []

    config_paths = {f.path for f in (snapshot.config.files if snapshot.config else [])}

    # Alternatives selections
    if any(p.startswith("/etc/alternatives/") for p in config_paths):
        env_notes.append((
            "### Alternatives selections",
            "The system has custom alternatives selections (e.g., default Python, Java, or editor). "
            "Installing the same packages in the image may not reproduce these selections. "
            "Review `alternatives --list` output and add `RUN alternatives --set ...` directives to the "
            "Containerfile if needed.",
        ))

    # Raw nftables rules outside firewalld
    _NFTABLES_PATHS = {"/etc/nftables.conf", "/etc/sysconfig/nftables.conf"}
    if _NFTABLES_PATHS & config_paths:
        env_notes.append((
            "### nftables rules",
            "This system has raw nftables rules outside of firewalld. These are included in the config "
            "tree but may conflict with firewalld if both are active. Review and consolidate firewall "
            "management before deployment.",
        ))

    # Complex network topologies
    _COMPLEX_NW_TYPES = ("bond", "vlan", "bridge", "team")
    if snapshot.network:
        complex_conns = [
            c for c in snapshot.network.connections
            if any(t in c.type.lower() for t in _COMPLEX_NW_TYPES)
        ]
        if complex_conns:
            names = ", ".join(f"`{c.name}` ({c.type})" for c in complex_conns)
            env_notes.append((
                "### Complex network topologies",
                "The following network connections use complex topologies (bonding, VLAN, bridging, "
                f"teaming): {names}. "
                "The NM profiles are included in the image, but deploy-time network configuration via "
                "kickstart should account for the physical topology of the target environment.",
            ))

    # Identity provider integration
    _IDP_PATHS = {"/etc/sssd/sssd.conf", "/etc/krb5.conf"}
    if _IDP_PATHS & config_paths:
        env_notes.append((
            "### Identity provider integration",
            "This system is integrated with an identity provider (SSSD/Kerberos). Config files are "
            "included, but Kerberos keytabs are machine-specific and excluded. After deployment: "
            "re-enroll the machine in the Kerberos realm, regenerate keytabs, and verify SSSD connectivity.",
        ))

    # NTP/Chrony configuration
    if "/etc/chrony.conf" in config_paths:
        env_notes.append((
            "### NTP/Chrony configuration",
            "Custom NTP servers are configured in `chrony.conf`. If deploying across multiple sites "
            "with different time sources, consider making the NTP server address a deploy-time parameter.",
        ))

    # Rsyslog forwarding
    if any(p.startswith("/etc/rsyslog.d/") for p in config_paths):
        env_notes.append((
            "### Rsyslog forwarding",
            "Custom rsyslog forwarding rules are configured. Log forwarding targets (syslog server "
            "addresses) are typically site-specific. For golden images deployed across multiple "
            "environments, consider injecting rsyslog forwarding configuration at deploy time.",
        ))

    # Always include the bootc /etc merge note
    env_notes.append((
        None,
        "**Note:** bootc uses a 3-way merge strategy for `/etc` during image updates. Local changes to "
        "`/etc` persist across updates, but the merge behavior has nuances — see the "
        "[bootc filesystem documentation](https://containers.github.io/bootc/filesystem.html) for details.",
    ))

    lines.append("## Environment-specific considerations")
    lines.append("")
    for heading, note in env_notes:
        if heading:
            lines.append(heading)
            lines.append("")
        lines.append(note)
        lines.append("")

    if snapshot.warnings:
        lines.append("## Items requiring manual intervention")
        lines.append("")
        for w in snapshot.warnings:
            lines.append(f"- {w.get('message') or '—'}")
        lines.append("")

    if snapshot.rpm and snapshot.rpm.leaf_packages is not None:
        leaf_set = set(snapshot.rpm.leaf_packages)
        auto_set = set(snapshot.rpm.auto_packages or [])
        dep_tree = snapshot.rpm.leaf_dep_tree or {}
        auto_pkgs = [p for p in snapshot.rpm.packages_added if p.name in auto_set]
        leaf_pkgs = [p for p in snapshot.rpm.packages_added if p.name in leaf_set]
        
        has_package_details = (auto_pkgs or dep_tree)
        if has_package_details:
            lines.append("## Package Details")
            lines.append("")
            
            if auto_pkgs:
                lines.append(f"### Dependencies ({len(auto_pkgs)})")
                lines.append("")
                lines.append("These packages are pulled in automatically by dnf. If the target image "
                             "produces a different dependency set, promote packages from this list to "
                             "the `dnf install` line.")
                lines.append("")
                for p in auto_pkgs:
                    prefix = "[EXCLUDED] " if not p.include else ""
                    lines.append(f"- {prefix}{p.name} {p.version}-{p.release}.{p.arch}")
                lines.append("")

            # Dependency tree view
            if dep_tree:
                total = len(leaf_pkgs) + len(auto_pkgs)
                lines.append(f"### Package Dependency Tree ({total} packages beyond base image)")
                lines.append("")
                lines.append(f"**{len(leaf_pkgs)} leaf packages** → {len(auto_pkgs)} dependencies")
                lines.append("")

                sorted_leaves = sorted(dep_tree.keys(), key=lambda k: -len(dep_tree.get(k, [])))
                for lf in sorted_leaves:
                    deps = dep_tree.get(lf, [])
                    if deps:
                        lines.append(f"**{lf}** ({len(deps)} deps)")
                        for i, d in enumerate(deps[:10]):
                            connector = "└──" if i == min(len(deps), 10) - 1 else "├──"
                            lines.append(f"  {connector} {d}")
                        if len(deps) > 10:
                            lines.append(f"  └── ... and {len(deps) - 10} more")
                        lines.append("")
                    else:
                        lines.append(f"**{lf}** (0 deps — installed independently)")
                        lines.append("")
            lines.append("")

    if snapshot.redactions:
        lines.append("## Redactions (secrets)")
        lines.append("")
        for r in snapshot.redactions:
            lines.append(f"- **{r.get('path') or ''}**: {r.get('pattern') or ''} — {r.get('remediation') or ''}")
        lines.append("")

    (output_dir / "audit-report.md").write_text("\n".join(lines))

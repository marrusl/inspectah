"""Markdown audit report renderer."""

from pathlib import Path

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot
from ._triage import compute_triage


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
    n_removed = len(snapshot.rpm.packages_removed) if snapshot.rpm else 0
    n_modified = len(snapshot.rpm.packages_modified) if snapshot.rpm else 0
    no_baseline = getattr(snapshot.rpm, "no_baseline", False) if snapshot.rpm else False
    n_config = len(snapshot.config.files) if snapshot.config else 0
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
    lines.append(f"- Packages removed: {n_removed}")
    if n_modified:
        lines.append(f"- Packages with modified configs: {n_modified}")
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
        lines.append("### Added")
        for p in snapshot.rpm.packages_added[:50]:
            lines.append(f"- {p.name} {p.version}-{p.release}.{p.arch}")
        if len(snapshot.rpm.packages_added) > 50:
            lines.append(f"- ... and {len(snapshot.rpm.packages_added) - 50} more")
        lines.append("")
        if snapshot.rpm.packages_removed:
            lines.append("### Removed (from baseline)")
            for p in snapshot.rpm.packages_removed:
                lines.append(f"- {p.name}")
            lines.append("")
        if snapshot.rpm.packages_modified:
            lines.append("### Modified (config changes detected by rpm -Va)")
            for p in snapshot.rpm.packages_modified:
                lines.append(f"- {p.name} {p.version}-{p.release}.{p.arch}")
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
            for name in snapshot.rpm.dnf_history_removed[:30]:
                lines.append(f"- {name}")
            if len(snapshot.rpm.dnf_history_removed) > 30:
                lines.append(f"- ... and {len(snapshot.rpm.dnf_history_removed) - 30} more")
            lines.append("")

    if snapshot.services and snapshot.services.state_changes:
        service_rows = [s for s in snapshot.services.state_changes if s.action != "unchanged"]
        if service_rows:
            lines.append("## Services")
            lines.append("")
            lines.append("| Unit | Current | Default | Action |")
            lines.append("|------|---------|---------|--------|")
            for s in service_rows:
                lines.append(f"| {s.unit} | {s.current_state} | {s.default_state} | {s.action} |")
            lines.append("")

    if snapshot.config and snapshot.config.files:
        lines.append("## Configuration Files")
        lines.append("")
        modified = [f for f in snapshot.config.files if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        unowned = [f for f in snapshot.config.files if f.kind == ConfigFileKind.UNOWNED]
        orphaned = [f for f in snapshot.config.files if f.kind == ConfigFileKind.ORPHANED]
        lines.append(f"- RPM-owned modified: {len(modified)}")
        lines.append(f"- Unowned: {len(unowned)}")
        if orphaned:
            lines.append(f"- Orphaned (from removed packages): {len(orphaned)}")
        for f in snapshot.config.files:
            flags_note = f" — rpm -Va flags: `{f.rpm_va_flags}`" if f.rpm_va_flags else ""
            pkg_note = f" (package: {f.package})" if f.package else ""
            lines.append(f"- `{f.path}` ({f.kind.value}{flags_note}{pkg_note})")
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

        if net.firewall_zones:
            lines.append("### Firewall zones (bake into image)")
            lines.append("")
            for z in net.firewall_zones:
                svc_str = ', '.join(z.services) or '—'
                port_str = ', '.join(z.ports) or '—'
                lines.append(f"**{z.name}:** services={svc_str} | ports={port_str} | rich rules={len(z.rich_rules)}")
                for r in z.rich_rules[:10]:
                    lines.append(f"  - `{r[:200]}`")
            lines.append("")

        if net.firewall_direct_rules:
            lines.append("### Firewall direct rules (bake into image)")
            lines.append("")
            for r in net.firewall_direct_rules:
                lines.append(f"- `{r.chain}` {r.ipv} table={r.table}: `{r.args}`")
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
            for e in snapshot.storage.fstab_entries[:30]:
                rec = _storage_recommendation(e.mount_point, e.fstype, e.device)
                lines.append(f"| `{e.device}` | `{e.mount_point}` | {e.fstype} | {rec} |")
            lines.append("")
        if snapshot.storage.lvm_info:
            lines.append("### LVM Layout")
            for lv in snapshot.storage.lvm_info[:20]:
                lines.append(f"- {lv.lv_name} ({lv.vg_name}) {lv.lv_size}")
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
            lines.append("### Existing systemd timers (vendor)")
            lines.append("")
            lines.append("| Timer | Schedule | ExecStart | Path |")
            lines.append("|-------|----------|-----------|------|")
            for t in vendor_timers:
                lines.append(f"| {t.name} | {t.on_calendar} | `{t.exec_start}` | `{t.path}` |")
            lines.append("")

        # Cron-converted timers
        if st.generated_timer_units:
            lines.append("### Cron-converted timers")
            lines.append("")
            for u in st.generated_timer_units[:20]:
                lines.append(f"- **{u.name}** — converted from `{u.source_path}` (cron: `{u.cron_expr}`)")
            lines.append("")

        # Raw cron jobs
        if st.cron_jobs:
            lines.append("### Cron jobs")
            lines.append("")
            for j in st.cron_jobs[:20]:
                lines.append(f"- `{j.path}` ({j.source})")
            lines.append("")

        # At jobs
        if st.at_jobs:
            lines.append("### At jobs")
            lines.append("")
            lines.append("| File | User | Command |")
            lines.append("|------|------|---------|")
            for a in st.at_jobs[:20]:
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
                lines.append(f"| {u.name} | `{u.image or '*none*'}` | `{u.path}` |")
            lines.append("")

        if ct.compose_files:
            lines.append("### Compose files")
            lines.append("")
            for c in ct.compose_files:
                lines.append(f"**`{c.path}`**")
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
        elf_items = [i for i in snapshot.non_rpm_software.items if i.get("lang")]
        venv_items = [i for i in snapshot.non_rpm_software.items if i.get("method") == "python venv"]
        git_items = [i for i in snapshot.non_rpm_software.items if i.get("method") == "git repository"]
        pip_items = [i for i in snapshot.non_rpm_software.items if i.get("method") == "pip dist-info"]
        other_items = [i for i in snapshot.non_rpm_software.items
                       if not i.get("lang") and i.get("method") not in ("python venv", "git repository", "pip dist-info")]

        if elf_items:
            lines.append("### Compiled binaries")
            lines.append("")
            lines.append("| Path | Language | Linking | Shared Libraries |")
            lines.append("|------|----------|---------|------------------|")
            for i in elf_items:
                linking = "static" if i.get("static") else "dynamic"
                libs = ", ".join(i.get("shared_libs", [])[:5])
                if len(i.get("shared_libs", [])) > 5:
                    libs += " ..."
                lines.append(f"| `{i.get('path','')}` | {i.get('lang','')} | {linking} | {libs or '—'} |")
            lines.append("")

        if venv_items:
            lines.append("### Python virtual environments")
            lines.append("")
            for v in venv_items:
                ssp_label = "**system-site-packages**" if v.get("system_site_packages") else "isolated"
                lines.append(f"#### `{v.get('path','')}` ({ssp_label})")
                lines.append("")
                pkgs = v.get("packages", [])
                if pkgs:
                    lines.append("| Package | Version |")
                    lines.append("|---------|---------|")
                    for p in pkgs[:20]:
                        lines.append(f"| {p.get('name','')} | {p.get('version','')} |")
                    if len(pkgs) > 20:
                        lines.append(f"| ... | +{len(pkgs)-20} more |")
                lines.append("")

        if git_items:
            lines.append("### Git-managed directories")
            lines.append("")
            lines.append("| Path | Remote | Branch | Commit |")
            lines.append("|------|--------|--------|--------|")
            for i in git_items:
                commit = i.get("git_commit", "")[:12]
                lines.append(f"| `{i.get('path','')}` | {i.get('git_remote','')} | {i.get('git_branch','')} | `{commit}` |")
            lines.append("")

        if pip_items:
            lines.append("### System pip packages")
            lines.append("")
            lines.append("| Package | Version | Path |")
            lines.append("|---------|---------|------|")
            for i in pip_items[:20]:
                lines.append(f"| {i.get('name','')} | {i.get('version','')} | `{i.get('path','')}` |")
            lines.append("")

        if other_items:
            lines.append("### Other non-RPM items")
            lines.append("")
            lines.append("| Path / Name | Version | Confidence | Method |")
            lines.append("|-------------|---------|------------|--------|")
            for i in other_items[:20]:
                pn = i.get("path") or i.get("name") or ""
                ver = i.get("version") or "—"
                conf = i.get("confidence") or "unknown"
                method = i.get("method") or "—"
                lines.append(f"| `{pn}` | {ver} | {conf} | {method} |")
            lines.append("")

    kb = snapshot.kernel_boot
    has_kernel = kb and (
        kb.cmdline or kb.sysctl_overrides or kb.non_default_modules
        or kb.modules_load_d or kb.modprobe_d or kb.dracut_conf
    )
    if has_kernel:
        lines.append("## Kernel and boot")
        lines.append("")
        if kb.cmdline:
            lines.append(f"- cmdline: `{kb.cmdline[:200]}`")
        if kb.grub_defaults:
            lines.append("- GRUB defaults present")

        if kb.non_default_modules:
            lines.append("")
            lines.append(f"### Non-default loaded modules ({len(kb.non_default_modules)})")
            lines.append("")
            lines.append("| Module | Size | Used by |")
            lines.append("|--------|------|---------|")
            for m in kb.non_default_modules[:30]:
                name = m.get("name", "?")
                size = m.get("size", "")
                used = m.get("used_by", "")
                lines.append(f"| `{name}` | {size} | {used} |")
            total = len(kb.loaded_modules or [])
            default_count = total - len(kb.non_default_modules)
            if default_count > 0:
                lines.append("")
                lines.append(f"_{default_count} module(s) at expected defaults (not shown)._")

        if kb.sysctl_overrides:
            lines.append("")
            lines.append(f"### Non-default sysctl values ({len(kb.sysctl_overrides)})")
            lines.append("")
            lines.append("| Key | Runtime | Default | Source |")
            lines.append("|-----|---------|---------|--------|")
            for s in kb.sysctl_overrides[:30]:
                key = s.get("key", "?")
                runtime = s.get("runtime", "?")
                default = s.get("default", "—")
                source = s.get("source", "")
                lines.append(f"| `{key}` | **{runtime}** | {default} | `{source}` |")

        for m in (kb.modules_load_d or [])[:20]:
            lines.append(f"- modules-load.d: `{m}`")
        for m in (kb.modprobe_d or [])[:20]:
            lines.append(f"- modprobe.d: `{m}`")
        for d in (kb.dracut_conf or [])[:20]:
            lines.append(f"- dracut: `{d}`")
        lines.append("")

    has_selinux = snapshot.selinux and (
        snapshot.selinux.mode or snapshot.selinux.custom_modules
        or snapshot.selinux.boolean_overrides or snapshot.selinux.audit_rules
        or snapshot.selinux.fips_mode or snapshot.selinux.pam_configs
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
            for m in snapshot.selinux.custom_modules[:20]:
                lines.append(f"  - `{m}`")
        non_default_bools = [b for b in (snapshot.selinux.boolean_overrides or []) if b.get("non_default")]
        if non_default_bools:
            lines.append(f"- **Non-default booleans** ({len(non_default_bools)}):")
            for b in non_default_bools[:30]:
                name = b.get("name", "?")
                cur = b.get("current", "?")
                dflt = b.get("default", "?")
                desc = b.get("description", "")
                lines.append(f"  - `{name}` = **{cur}** (default: {dflt}) — {desc}")
        unchanged_count = len(snapshot.selinux.boolean_overrides or []) - len(non_default_bools)
        if unchanged_count > 0:
            lines.append(f"- Unchanged booleans: {unchanged_count} (at default values)")
        if snapshot.selinux.audit_rules:
            lines.append(f"- Audit rule files: {len(snapshot.selinux.audit_rules)}")
            for a in snapshot.selinux.audit_rules[:10]:
                lines.append(f"  - `{a}`")
        if snapshot.selinux.pam_configs:
            lines.append(f"- PAM config files: {len(snapshot.selinux.pam_configs)}")
            for p in snapshot.selinux.pam_configs[:10]:
                lines.append(f"  - `{p}`")
        lines.append("")

    has_users = snapshot.users_groups and (
        snapshot.users_groups.users or snapshot.users_groups.groups
        or snapshot.users_groups.sudoers_rules or snapshot.users_groups.ssh_authorized_keys_refs
    )
    if has_users:
        lines.append("## Users and groups")
        lines.append("")
        for u in (snapshot.users_groups.users or [])[:30]:
            shell = u.get("shell") or ""
            home = u.get("home") or ""
            lines.append(f"- User: **{u.get('name') or ''}** (uid {u.get('uid') or ''}, home `{home}`, shell `{shell}`)")
        for g in (snapshot.users_groups.groups or [])[:30]:
            members = g.get("members") or []
            mem_str = f", members: {', '.join(members)}" if members else ""
            lines.append(f"- Group: **{g.get('name') or ''}** (gid {g.get('gid') or ''}{mem_str})")
        if snapshot.users_groups.sudoers_rules:
            lines.append("")
            lines.append("### Sudoers rules")
            for r in snapshot.users_groups.sudoers_rules[:20]:
                lines.append(f"- `{r}`")
        if snapshot.users_groups.ssh_authorized_keys_refs:
            lines.append("")
            lines.append("### SSH authorized_keys (flagged for manual handling)")
            for ref in snapshot.users_groups.ssh_authorized_keys_refs[:20]:
                lines.append(f"- User **{ref.get('user') or ''}**: `{ref.get('path') or ''}`")
        lines.append("")

    lines.append("## Data Migration Plan (/var)")
    lines.append("")
    lines.append("Content under `/var` is seeded at initial bootstrap and **not updated** by subsequent bootc deployments.")
    lines.append("`tmpfiles.d` snippets ensure expected directories exist on every boot.")
    lines.append("Review application data under `/var/lib`, `/var/log`, `/var/data` for separate migration strategies.")
    lines.append("")

    if snapshot.warnings:
        lines.append("## Items requiring manual intervention")
        lines.append("")
        for w in snapshot.warnings:
            lines.append(f"- {w.get('message') or '—'}")
        lines.append("")

    if snapshot.redactions:
        lines.append("## Redactions (secrets)")
        lines.append("")
        for r in snapshot.redactions:
            lines.append(f"- **{r.get('path') or ''}**: {r.get('pattern') or ''} — {r.get('remediation') or ''}")
        lines.append("")

    (output_dir / "audit-report.md").write_text("\n".join(lines))

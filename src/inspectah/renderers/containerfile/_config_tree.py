"""Config tree file-writing and inventory comment generation."""

import sys
from pathlib import Path

from ...schema import ConfigFileKind, InspectionSnapshot, RedactionFinding
from .._triage import _QUADLET_PREFIX
from ._helpers import _dhcp_connection_paths, _operator_kargs, _summarise_diff


def _safe_write_file(dest: Path, content: str) -> None:
    """Write content to dest, handling directory/file collisions gracefully.

    If dest already exists as a directory (from a prior entry creating a
    subdirectory at the same path), skip the write and warn.  If any
    ancestor of dest already exists as a regular file (blocking mkdir),
    skip the write and warn.
    """
    if dest.is_dir():
        print(
            f"inspectah: warning: skipping config file write — "
            f"path is already a directory: {dest}",
            file=sys.stderr,
        )
        return
    try:
        dest.parent.mkdir(parents=True, exist_ok=True)
    except (NotADirectoryError, FileExistsError):
        # A component in the parent chain is an existing regular file
        print(
            f"inspectah: warning: skipping config file write — "
            f"parent path conflict: {dest}",
            file=sys.stderr,
        )
        return
    dest.write_text(content)


def write_config_tree(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write all config files from snapshot to output_dir/config/ preserving paths."""
    config_dir = output_dir / "config"
    dhcp_paths = _dhcp_connection_paths(snapshot)

    if snapshot.config and snapshot.config.files:
        for entry in snapshot.config.files:
            if not entry.include:
                continue
            rel = entry.path.lstrip("/")
            if not rel:
                continue
            if rel in dhcp_paths:
                continue
            # Quadlet files are written to quadlet/ and COPYed separately
            if rel.startswith(_QUADLET_PREFIX):
                continue
            dest = config_dir / rel
            _safe_write_file(dest, entry.content or "")

    if snapshot.rpm and snapshot.rpm.repo_files:
        for repo in snapshot.rpm.repo_files:
            if not repo.include or not repo.path:
                continue
            dest = config_dir / repo.path
            _safe_write_file(dest, repo.content or "")

    if snapshot.rpm and snapshot.rpm.gpg_keys:
        for key in snapshot.rpm.gpg_keys:
            if not key.include or not key.path:
                continue
            dest = config_dir / key.path
            _safe_write_file(dest, key.content or "")

    # Firewalld zones and direct rules
    if snapshot.network:
        for z in snapshot.network.firewall_zones:
            if not z.include:
                continue
            if z.path:
                dest = config_dir / z.path
                _safe_write_file(dest, z.content)
        included_direct = [r for r in snapshot.network.firewall_direct_rules if r.include]
        if included_direct:
            import xml.etree.ElementTree as ET
            direct_el = ET.Element("direct")
            for r in included_direct:
                rule_el = ET.SubElement(direct_el, "rule")
                rule_el.set("priority", r.priority)
                rule_el.set("table", r.table)
                rule_el.set("ipv", r.ipv)
                rule_el.set("chain", r.chain)
                rule_el.text = r.args
            dest = config_dir / "etc/firewalld/direct.xml"
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text('<?xml version="1.0" encoding="utf-8"?>\n'
                            + ET.tostring(direct_el, encoding="unicode") + "\n")

        # Static NM connection profiles (baked into image)
        for c in snapshot.network.connections:
            if c.method == "static" and c.path:
                dest = config_dir / c.path
                if not dest.exists():
                    _safe_write_file(dest, "")

    # Custom tuned profiles
    if snapshot.kernel_boot and snapshot.kernel_boot.tuned_custom_profiles:
        for tp in snapshot.kernel_boot.tuned_custom_profiles:
            if tp.path:
                dest = config_dir / tp.path
                _safe_write_file(dest, tp.content or "")

    # Systemd drop-in overrides — write to both config/ (for Containerfile COPY)
    # and drop-ins/ (for the file browser tree to show them as a dedicated section)
    if snapshot.services and snapshot.services.drop_ins:
        dropins_dir = output_dir / "drop-ins"
        for di in snapshot.services.drop_ins:
            if not di.include:
                continue
            dest = config_dir / di.path
            _safe_write_file(dest, di.content or "")
            dropin_dest = dropins_dir / di.path
            _safe_write_file(dropin_dest, di.content or "")

    # Systemd timer units: cron-generated and existing local timers
    st = snapshot.scheduled_tasks
    if st and (st.generated_timer_units or st.systemd_timers):
        systemd_dir = config_dir / "etc/systemd/system"
        systemd_dir.mkdir(parents=True, exist_ok=True)
        for u in st.generated_timer_units:
            if not u.include:
                continue
            (systemd_dir / f"{u.name}.timer").write_text(u.timer_content)
            (systemd_dir / f"{u.name}.service").write_text(u.service_content)
        for t in st.systemd_timers:
            if t.source == "local":
                if t.name and t.timer_content:
                    (systemd_dir / f"{t.name}.timer").write_text(t.timer_content)
                if t.name and t.service_content:
                    (systemd_dir / f"{t.name}.service").write_text(t.service_content)

    # Quadlet units
    if snapshot.containers and snapshot.containers.quadlet_units:
        quadlet_dir = output_dir / "quadlet"
        quadlet_dir.mkdir(parents=True, exist_ok=True)
        for u in snapshot.containers.quadlet_units:
            if not u.include:
                continue
            if u.name and u.content:
                (quadlet_dir / u.name).write_text(u.content)

    # Dotenv / secret files found under /opt
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        for entry in snapshot.non_rpm_software.env_files:
            if not entry.include:
                continue
            rel = entry.path.lstrip("/")
            if not rel:
                continue
            dest = config_dir / rel
            _safe_write_file(dest, entry.content or "")

    # Non-RPM software files
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        for item in snapshot.non_rpm_software.items:
            if not item.include:
                continue
            # Items with a "files" dict (npm/yarn/gem lockfile dirs)
            if item.path and item.files and isinstance(item.files, dict):
                rel = item.path.lstrip("/")
                if not rel:
                    continue
                dest = config_dir / rel
                if dest.is_file():
                    dest.unlink()  # remove conflicting file to create directory
                dest.mkdir(parents=True, exist_ok=True)
                for fname, fcontent in item.files.items():
                    _safe_write_file(dest / fname, fcontent)
            # Items with simple "content" (requirements.txt, single files)
            elif item.path and item.content:
                rel = item.path.lstrip("/")
                if not rel:
                    continue
                dest = config_dir / rel
                _safe_write_file(dest, item.content)

    # User/group provisioning files — strategy-aware
    ug = snapshot.users_groups
    if ug:
        # Sysusers: write /usr/lib/sysusers.d/inspectah-users.conf
        sysusers_users = [u for u in (ug.users or []) if u.get("strategy") == "sysusers" and u.get("include", True)]
        sysusers_groups = [g for g in (ug.groups or []) if g.get("strategy") == "sysusers" and g.get("include", True)]
        if sysusers_users or sysusers_groups:
            sysusers_dir = config_dir / "usr/lib/sysusers.d"
            sysusers_dir.mkdir(parents=True, exist_ok=True)
            sysusers_lines = ["# Generated by inspectah — service accounts created at boot by systemd-sysusers"]
            for g in sysusers_groups:
                sysusers_lines.append(f"g {g.get('name', '')} {g.get('gid', '-')}")
            for u in sysusers_users:
                uid = u.get("uid", "-")
                gecos = u.get("name", "")
                home = u.get("home", "-")
                shell = u.get("shell", "-")
                gid_ref = f"{u.get('gid', '-')}" if u.get("gid") else "-"
                sysusers_lines.append(f"u {u.get('name', '')} {uid}:{gid_ref} \"{gecos}\" {home} {shell}")
            (sysusers_dir / "inspectah-users.conf").write_text("\n".join(sysusers_lines) + "\n")

            # Tmpfiles.d for sysusers home directories
            tmpfiles_dir = config_dir / "etc/tmpfiles.d"
            tmpfiles_dir.mkdir(parents=True, exist_ok=True)
            home_lines = ["# Home directories for sysusers-managed accounts"]
            for u in sysusers_users:
                home = u.get("home", "")
                name = u.get("name", "")
                if home and home != "/" and name:
                    home_lines.append(f"d {home} 0755 {name} {name} -")
            if len(home_lines) > 1:
                tmpfiles_path = tmpfiles_dir / "inspectah-sysusers-homes.conf"
                tmpfiles_path.write_text("\n".join(home_lines) + "\n")

        # Blueprint TOML — only when at least one user has blueprint strategy
        blueprint_users = [u for u in (ug.users or []) if u.get("strategy") == "blueprint" and u.get("include", True)]
        blueprint_groups = [g for g in (ug.groups or []) if g.get("strategy") == "blueprint" and g.get("include", True)]
        if blueprint_users or blueprint_groups:
            toml_lines = ["# Generated by inspectah — bootc-image-builder customization", ""]
            for g in blueprint_groups:
                toml_lines.append("[[customizations.group]]")
                toml_lines.append(f"name = \"{g.get('name', '')}\"")
                toml_lines.append(f"gid = {g.get('gid', 0)}")
                toml_lines.append("")
            for u in blueprint_users:
                toml_lines.append("[[customizations.user]]")
                toml_lines.append(f"name = \"{u.get('name', '')}\"")
                if u.get("uid"):
                    toml_lines.append(f"uid = {u['uid']}")
                if u.get("gid"):
                    toml_lines.append(f"gid = {u['gid']}")
                if u.get("home"):
                    toml_lines.append(f"home = \"{u['home']}\"")
                if u.get("shell"):
                    toml_lines.append(f"shell = \"{u['shell']}\"")
                toml_lines.append("")
            (output_dir / "inspectah-users.toml").write_text("\n".join(toml_lines))

    # Kernel module / sysctl / dracut configs
    if snapshot.kernel_boot:
        for section_list in (
            snapshot.kernel_boot.modules_load_d,
            snapshot.kernel_boot.modprobe_d,
            snapshot.kernel_boot.dracut_conf,
        ):
            for entry in (section_list or []):
                kpath = entry.path
                content = entry.content
                if kpath:
                    dest = config_dir / kpath
                    _safe_write_file(dest, content)

    if snapshot.kernel_boot and snapshot.kernel_boot.sysctl_overrides:
        included_sysctls = [s for s in snapshot.kernel_boot.sysctl_overrides if s.include]
        if included_sysctls:
            sysctl_dir = config_dir / "etc/sysctl.d"
            sysctl_dir.mkdir(parents=True, exist_ok=True)
            sysctl_lines = ["# Non-default sysctl values detected by inspectah"]
            for s in included_sysctls:
                sysctl_lines.append(f"{s.key} = {s.runtime}")
            (sysctl_dir / "99-inspectah.conf").write_text("\n".join(sysctl_lines) + "\n")

    # Kernel arguments: write bootc-native kargs.d TOML drop-in
    if snapshot.kernel_boot and snapshot.kernel_boot.cmdline:
        safe_kargs = _operator_kargs(snapshot.kernel_boot.cmdline)
        if safe_kargs:
            kargs_dir = config_dir / "usr/lib/bootc/kargs.d"
            kargs_dir.mkdir(parents=True, exist_ok=True)
            kargs_quoted = ", ".join(f'"{k}"' for k in safe_kargs)
            (kargs_dir / "inspectah-migrated.toml").write_text(
                f"# Generated by inspectah — review and remove args that belong to\n"
                f"# the bootloader/base image rather than this specific image.\n"
                f"kargs = [{kargs_quoted}]\n"
            )

    # tmpfiles.d for /var (and home) directory structure
    tmpfiles_dir = config_dir / "etc/tmpfiles.d"
    tmpfiles_dir.mkdir(parents=True, exist_ok=True)
    tmpfiles_lines = [
        "# Generated by inspectah: directories created on every boot.",
        "# /var is seeded at initial bootstrap only; bootc does not update it.",
        "# Add d lines for application dirs under /var or /home as needed.",
    ]
    if snapshot.users_groups and snapshot.users_groups.users:
        for u in snapshot.users_groups.users[:20]:
            if not u.get("include", True):
                continue
            name = u.get("name", "")
            if name and name != "root":
                tmpfiles_lines.append(f"d /home/{name} 0755 {name} - -")
    if len(tmpfiles_lines) <= 3:
        tmpfiles_lines.append("d /var/lib/app 0755 root root -")
    (tmpfiles_dir / "inspectah-var.conf").write_text("\n".join(tmpfiles_lines) + "\n")


def config_copy_roots(config_dir: Path):
    """Return sorted list of non-empty top-level subdirectory names under config_dir.

    'tmp' is excluded — those files get individual COPY lines to /tmp/.
    This is called after write_config_tree so we copy exactly what was written.
    """
    roots = []
    try:
        for d in sorted(config_dir.iterdir()):
            if not d.is_dir() or d.name == "tmp":
                continue
            # Only include if the directory is non-empty
            if any(True for _ in d.rglob("*") if _.is_file()):
                roots.append(d.name)
    except (PermissionError, OSError):
        pass
    return roots


def config_inventory_comment(snapshot: InspectionSnapshot, dhcp_paths: set) -> list:
    """Build a block comment listing everything that will be in the consolidated COPY."""
    lines = []

    # Config files (modified, unowned, orphaned)
    if snapshot.config and snapshot.config.files:
        config_entries = [f for f in snapshot.config.files if f.include and f.path.lstrip("/") not in dhcp_paths]
        modified = [f for f in config_entries if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        unowned = [f for f in config_entries if f.kind == ConfigFileKind.UNOWNED]
        orphaned = [f for f in config_entries if f.kind == ConfigFileKind.ORPHANED]
        if modified:
            lines.append(f"# Modified RPM-owned configs ({len(modified)}):")
            for f in modified:
                rel = f.path.lstrip("/")
                if f.diff_against_rpm and f.diff_against_rpm.strip():
                    pkg_label = f.package or "RPM"
                    lines.append(f"#   {rel} (modified from {pkg_label} default):")
                    changes = _summarise_diff(f.diff_against_rpm)
                    for ch in changes[:5]:
                        lines.append(f"#     - {ch}")
                    remaining = len(changes) - 5
                    if remaining > 0:
                        lines.append(f"#     ... and {remaining} more change(s)")
                    lines.append(f"#     See audit-report.md or report.html for full diff")
                else:
                    flags = f" (rpm -Va: {f.rpm_va_flags})" if f.rpm_va_flags else ""
                    lines.append(f"#   {rel}{flags}")
        if unowned:
            lines.append(f"# Unowned configs ({len(unowned)}):")
            for f in unowned[:10]:
                lines.append(f"#   {f.path.lstrip('/')}")
            if len(unowned) > 10:
                lines.append(f"#   ... and {len(unowned) - 10} more")
        if orphaned:
            lines.append(f"# Orphaned configs from removed packages ({len(orphaned)}):")
            for f in orphaned[:5]:
                lines.append(f"#   {f.path.lstrip('/')}")

    # Tied items (across all variant-bearing sections)
    tied_items = []
    if snapshot.config and snapshot.config.files:
        tied_configs = [f for f in snapshot.config.files if getattr(f, "tie_winner", False)]
        for f in tied_configs:
            path_variants = [v for v in snapshot.config.files if v.path == f.path]
            tied_items.append((f.path.lstrip("/"), "config", f.fleet, len(path_variants)))
    if snapshot.services and snapshot.services.drop_ins:
        tied_dropins = [d for d in snapshot.services.drop_ins if getattr(d, "tie_winner", False)]
        for d in tied_dropins:
            path_variants = [v for v in snapshot.services.drop_ins if v.path == d.path]
            tied_items.append((d.path.lstrip("/"), "drop-in", d.fleet, len(path_variants)))
    if snapshot.containers:
        if snapshot.containers.quadlet_units:
            tied_quads = [q for q in snapshot.containers.quadlet_units if getattr(q, "tie_winner", False)]
            for q in tied_quads:
                path_variants = [v for v in snapshot.containers.quadlet_units if v.path == q.path]
                tied_items.append((q.path.lstrip("/"), "quadlet", q.fleet, len(path_variants)))
        if snapshot.containers.compose_files:
            tied_compose = [c for c in snapshot.containers.compose_files if getattr(c, "tie_winner", False)]
            for c in tied_compose:
                path_variants = [v for v in snapshot.containers.compose_files if v.path == c.path]
                tied_items.append((c.path.lstrip("/"), "compose", c.fleet, len(path_variants)))
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        tied_envs = [f for f in snapshot.non_rpm_software.env_files if getattr(f, "tie_winner", False)]
        for f in tied_envs:
            path_variants = [v for v in snapshot.non_rpm_software.env_files if v.path == f.path]
            tied_items.append((f.path.lstrip("/"), "env", f.fleet, len(path_variants)))

    if tied_items:
        lines.append(f"# Tied items resolved by content-hash tiebreaker ({len(tied_items)}):")
        for path, item_type, fleet, variant_count in tied_items:
            lines.append(f"#   {path}  ({item_type}, {fleet.count}/{fleet.total} hosts each, {variant_count} variants)")
        lines.append("#   See merge-notes.md for tie details")
        lines.append("#   Review in report.html or run `inspectah refine` to change selection")

    # Repo files
    if snapshot.rpm and snapshot.rpm.repo_files:
        included_repos = [r for r in snapshot.rpm.repo_files if r.include]
        excluded_repos = [r for r in snapshot.rpm.repo_files if not r.include]
        if included_repos:
            lines.append(f"# Repo files ({len(included_repos)}):")
            for r in included_repos[:5]:
                lines.append(f"#   {r.path}")
        for r in excluded_repos:
            lines.append(f"# Excluded repo: {r.path}")

    # Firewall
    net = snapshot.network
    included_zones = [z for z in net.firewall_zones if z.include] if net else []
    included_direct = [r for r in net.firewall_direct_rules if r.include] if net else []
    if included_zones:
        lines.append(f"# Firewall zones ({len(included_zones)}): "
                     + ", ".join(z.name for z in included_zones[:5]))
    if included_direct:
        lines.append(f"# Firewall direct rules: etc/firewalld/direct.xml")

    # Static NM connections
    if net:
        static_conns = [c for c in net.connections if c.method == "static"]
        if static_conns:
            lines.append(f"# Static NM connections ({len(static_conns)}): "
                         + ", ".join(c.name for c in static_conns[:5]))

    # Timers
    st = snapshot.scheduled_tasks
    if st:
        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        if local_timers:
            lines.append(f"# Local systemd timers ({len(local_timers)}): "
                         + ", ".join(t.name for t in local_timers[:5]))
        incl_gen_timers = [u for u in st.generated_timer_units if u.include]
        if incl_gen_timers:
            lines.append(f"# Cron-converted timers ({len(incl_gen_timers)}): "
                         + ", ".join(u.name for u in incl_gen_timers[:5]))

    # Kernel
    kb = snapshot.kernel_boot
    if kb:
        if kb.cmdline:
            safe_kargs = _operator_kargs(kb.cmdline)
            if safe_kargs:
                lines.append(f"# kargs.d: usr/lib/bootc/kargs.d/inspectah-migrated.toml ({len(safe_kargs)} arg(s))")
        if kb.modules_load_d:
            lines.append(f"# modules-load.d: {len(kb.modules_load_d)} file(s)")
        if kb.modprobe_d:
            lines.append(f"# modprobe.d: {len(kb.modprobe_d)} file(s)")
        if kb.dracut_conf:
            lines.append(f"# dracut.conf.d: {len(kb.dracut_conf)} file(s)")
        if kb.sysctl_overrides:
            lines.append(f"# sysctl overrides: etc/sysctl.d/99-inspectah.conf ({len(kb.sysctl_overrides)} value(s))")

    # Systemd drop-in overrides
    if snapshot.services and snapshot.services.drop_ins:
        included = [d for d in snapshot.services.drop_ins if d.include]
        if included:
            units = sorted(set(d.unit for d in included))
            lines.append(f"# Systemd drop-in overrides ({len(included)}): "
                         + ", ".join(units[:5])
                         + (" ..." if len(units) > 5 else ""))

    # SELinux audit rules
    if snapshot.selinux and snapshot.selinux.audit_rules:
        lines.append(f"# Audit rules: {len(snapshot.selinux.audit_rules)} file(s)")

    # Tuned profiles
    kb = snapshot.kernel_boot
    if kb and (kb.tuned_active or kb.tuned_custom_profiles):
        parts = [f"Tuned: active={kb.tuned_active or '(none)'}"]
        if kb.tuned_custom_profiles:
            parts.append(f"{len(kb.tuned_custom_profiles)} custom profile(s)")
        lines.append(f"# {', '.join(parts)}")

    # tmpfiles.d (always written)
    lines.append("# tmpfiles.d: etc/tmpfiles.d/inspectah-var.conf")

    return lines


# ---------------------------------------------------------------------------
# redacted/ directory — placeholder files for excluded secrets
# ---------------------------------------------------------------------------

_REGENERATE_TEMPLATE = """\
# REDACTED by inspectah — auto-generated credential
# Original path: {path}
# Action: no action needed — this file is regenerated automatically on the target system
# See secrets-review.md for details
"""

_PROVISION_TEMPLATE = """\
# REDACTED by inspectah — sensitive file detected
# Original path: {path}
# Action: provision this file on the target system from your secrets management process
# See secrets-review.md for details
"""


def write_redacted_dir(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write .REDACTED placeholder files for excluded secrets."""
    for finding in snapshot.redactions:
        if not isinstance(finding, RedactionFinding):
            continue
        if finding.source != "file" or finding.kind != "excluded":
            continue
        rel = finding.path.lstrip("/")
        if not rel:
            continue
        redacted_dir = output_dir / "redacted"
        dest = redacted_dir / (rel + ".REDACTED")
        dest.parent.mkdir(parents=True, exist_ok=True)
        if finding.remediation == "regenerate":
            content = _REGENERATE_TEMPLATE.format(path=finding.path)
        else:
            content = _PROVISION_TEMPLATE.format(path=finding.path)
        dest.write_text(content)
